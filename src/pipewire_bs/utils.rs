use crate::pipewire_bs::abstractions;
use crate::pipewire_bs::abstractions::{get_port_node_id, is_not_default, port_is_input};
use crate::pipewire_bs::input_port::InputPort;
use crate::pipewire_bs::link::LinkEndpoints;
use crate::pipewire_bs::state::State;
use crate::pipewire_bs::tracked_link::TrackedLink;
use crate::{INSTANT_REJECT_CHANNEL_FILTER, INSTANT_REJECT_FORMAT_FILTER, INSTANT_REJECT_NAME_FILTER, INSTANT_REJECT_PATH_FILTER};
use pipewire::core::{CoreRc, Listener, PW_ID_CORE};
use pipewire::metadata::Metadata;
use pipewire::registry::{GlobalObject, RegistryRc};
use pipewire::spa::utils::dict::DictRef;
use pipewire::types::ObjectType;
use std::cell::RefCell;
use std::rc::Rc;

static DEBUG: bool = false;

/// Determines whether a PipeWire global object should be ignored (filtered out)
/// based on its type and properties.
///
/// The object is ignored if any of the following conditions hold:
pub fn should_ignore(global: &GlobalObject<&DictRef>) -> bool {
  if DEBUG { println!("Check: Must be a port") }
  if global.type_ != ObjectType::Port { return true }
  if DEBUG { println!("Check: Props exists") }
  if global.props.is_none() { return true }

  let props = global.props.unwrap();
  if DEBUG { println!("Check: Props has entries") }
  if props.is_empty() { return true }

  let port_name = props.get("port.name").unwrap();
  let port_format = props.get("format.dsp");
  let port_channel = props.get("audio.channel");
  let port_path = props.get("object.path").unwrap();
  if DEBUG { println!("Check: Blocked names") }
  if INSTANT_REJECT_NAME_FILTER.iter().any(|f| port_name.starts_with(f) && !port_path.contains("vrlink")) { return true }

  if port_format.is_some() {
    if DEBUG { println!("Check: Blocked formats") }
    if INSTANT_REJECT_FORMAT_FILTER.iter().any(|f| port_format.unwrap().contains(f)) { return true }
  }

  if DEBUG { println!("Check: Must have a channel") }
  let port_channel = if port_channel.is_none() { return true } else { port_channel.unwrap() };

  if DEBUG { println!("Check: Must not be banned channel") }
  if INSTANT_REJECT_CHANNEL_FILTER.iter().any(|f| port_channel.starts_with(f)) { return true }

  if DEBUG { println!("Check: Must not be banned path") }
  if INSTANT_REJECT_PATH_FILTER.iter().any(|f| port_path.starts_with(f)) { return true }

  false
}


/// Builds a callback that requests a PipeWire core sync.
///
/// The returned closure captures clones of `core` and `state`, so the
/// originals do not need to outlive it. When invoked, it checks whether a
/// sync is already pending; if not, it issues `core.sync(0)` and records the
/// returned sequence number in `state.pending_sync`. If a sync is already in
/// flight, the call is a no-op, preventing redundant round-trips while waiting
/// for the server to acknowledge the previous one.
///
/// On failure to issue the sync request, an error is written to `stderr` and
/// `pending_sync` is left unchanged.
///
pub fn create_sync_callback(core: &CoreRc, state: &Rc<RefCell<State>>) -> impl Fn() {
  let core = core.clone();
  let state = state.clone();
  move || {
    let mut s = state.borrow_mut();
    if s.pending_sync.is_none() {
      match core.sync(0) {
        Ok(seq) => s.pending_sync = Some(seq),
        Err(e) => eprintln!("Failed to sync internal graph status with pipewire: {e}"),
      }
    }
  }
}

pub fn create_core_listener(core: &CoreRc, state: &Rc<RefCell<State>>) -> Listener {
  let state = state.clone();
  let core_cb = core.clone();
  core
    .add_listener_local()
    .done(move |id, seq| {
      if id != PW_ID_CORE {
        return;
      }
      let mut s = state.borrow_mut();
      if s.pending_sync == Some(seq) {
        s.pending_sync = None;
        s.report_changes();
        s.autoconnect_disconnected(&core_cb);
      }
    })
    .register()
}

/// Extracts the `name` from a metadata value such as
/// `{ "name": "alsa_output.pci-0000_00_1f.3.analog-stereo" }`.
pub fn parse_default_target_name(value: &str) -> Option<String> {
  let idx = value.find("\"name\"")?;
  let rest = value[idx + "\"name\"".len()..].trim_start();
  let rest = rest.strip_prefix(':')?.trim_start().strip_prefix('"')?;
  let end = rest.find('"')?;
  Some(rest[..end].to_string())
}

/// Handles a newly announced PipeWire `Metadata` global object.
///
/// Only the default metadata object is of interest. Any other metadata object is ignored.
///
pub fn handle_metadata(state_add: &Rc<RefCell<State>>, registry_bind: &RegistryRc, sync_add: &Rc<impl Fn() + 'static>, global: &GlobalObject<&DictRef>, props: &DictRef) -> bool {
  if is_not_default(props) { return true };

  let metadata: Metadata = match registry_bind.bind(global) {
    Ok(m) => m,
    Err(e) => {
      eprintln!("failed to bind metadata {}: {e}", global.id);
      return true;
    }
  };

  let state_meta = state_add.clone();
  let sync_meta = sync_add.clone();
  let listener = metadata
    .add_listener_local()
    .property(move |_subject, key, _type, value| {
      if key == Some("default.audio.sink") {
        let name = value.and_then(parse_default_target_name);
        println!("default audio sink: {name:?}");
        state_meta.borrow_mut().default_sink_name = name;
        sync_meta();
      }
      0
    })
    .register();
  state_add.borrow_mut().default_metadata = Some((metadata, listener));
  false
}

/// Handles a newly discovered PipeWire node by recording its name in the shared state.
///
/// If the node's properties contain a `node.name` entry, the name is stored in
/// the `nodes` map of the shared [`State`] under the node's global `id`, and the
/// provided synchronization callback is invoked to notify listeners of the change.
pub fn handle_node(state_add: &Rc<RefCell<State>>, sync_add: &Rc<impl Fn()>, global: &GlobalObject<&DictRef>, props: &DictRef) {
  if let Some(name) = props.get("node.name") {
    state_add.borrow_mut().nodes.insert(global.id, name.to_string());
    sync_add();
  }
}

/// Handles the appearance of a new PipeWire link global object.
///
/// Attempts to extract the link's endpoints from `props` and, if both endpoints
/// belong to nodes that are already tracked in `state_add`, binds the link
/// through `registry_bind`, attaches an individual listener to it, and records
/// it in the state's `tracked_links` map keyed by the global's id. Finally,
/// `sync_add` is invoked so the caller can propagate the updated link set.
///
/// The function returns early without side effects if:
/// - the endpoints cannot be parsed from `props`,
/// - either endpoint is not tracked in the current state, or
/// - binding the link via the registry fails.
pub fn handle_link(state_add: &Rc<RefCell<State>>, registry_bind: &RegistryRc, sync_add: &Rc<impl Fn()>, global: &GlobalObject<&DictRef>, props: &DictRef) -> bool {
  let Some(endpoints) = LinkEndpoints::from_props(props) else { return true };
  if abstractions::endpoint_not_tracked(endpoints, &state_add) { return true; }
  let Some(link) = abstractions::bind_link(&registry_bind, global) else { return true; };

  let listener = abstractions::create_individual_link_listener(global, &link);
  state_add
    .borrow_mut()
    .tracked_links
    .insert(global.id, TrackedLink::new(endpoints, link, listener));
  sync_add();
  false
}

pub fn handle_port(state_add: &Rc<RefCell<State>>, sync_add: &Rc<impl Fn()>, global: &GlobalObject<&DictRef>, props: &DictRef) {
  let mut changed = false;

  if port_is_input(props) {
    let node = get_port_node_id(props);
    if let (Some(node), Some(channel)) = (node, props.get("audio.channel")) {
      state_add
        .borrow_mut()
        .input_ports
        .insert(global.id, InputPort { node, channel: channel.to_string() });
      changed = true;
    }
  }

  if !should_ignore(global) {
    state_add.borrow_mut().tracked_objects.insert(global.id, global.to_owned());
    changed = true;
  }

  if changed { sync_add(); }
}