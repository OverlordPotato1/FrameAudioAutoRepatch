use crate::pipewire_bs::connection_state::ConnectionState;
use crate::pipewire_bs::input_port::InputPort;
use crate::pipewire_bs::tracked_link::TrackedLink;
use pipewire::__properties__ as properties;
use pipewire::core::Core;
use pipewire::link::Link;
use pipewire::metadata::{Metadata, MetadataListener};
use pipewire::properties::PropertiesBox;
use pipewire::registry::{GlobalObject, Registry};
use pipewire::spa::utils::result::AsyncSeq;
use std::collections::{HashMap};
use crate::pipewire_bs::link::LinkEndpoints;

/// Internal state for tracking PipeWire global objects and the links between them.
///
#[derive(Default)]
pub struct State {
  /// Objects tracked by this manager, keyed by their global object ID.
  ///
  /// Each entry maps a server-assigned ID to the [`GlobalObject`] holding the
  /// object's property state. Entries are added when the server announces a
  /// new global and removed when the global is withdrawn.
  pub(crate) tracked_objects: HashMap<u32, GlobalObject<PropertiesBox>>,
  /// Links currently being tracked, keyed by their unique link identifier.
  pub(crate) tracked_links: HashMap<u32, TrackedLink>,
  /// Tracks the most recently reported [`ConnectionState`] for each connection,
  last_reported: HashMap<u32, ConnectionState>,
  /// All known nodes, keyed by global id, mapped to their `node.name`.
  pub(crate) nodes: HashMap<u32, String>,
  /// All audio input ports in the graph, keyed by port id.
  pub(crate) input_ports: HashMap<u32, InputPort>,

  pub(crate) historical_output_sink_names: HashMap<String, bool>,
  /// Proxy + listener for the "default" metadata object; kept alive to receive updates.
  pub(crate) default_metadata: Option<(Metadata, MetadataListener)>,
  /// Links we created ourselves; kept alive so the proxies are not destroyed.
  auto_links: Vec<Link>,
  /// Ports for which a link has been requested but not yet announced by the server.
  pending_autoconnect: HashMap<u32, u8>,
  /// Sequence number of the sync operation currently in flight, if any.
  /// Sequence number of the sync operation currently in flight, if any.
  ///
  /// Set when an asynchronous sync is initiated and cleared once that
  /// operation completes. While this is `Some`, the value identifies the
  /// outstanding request so its completion can be matched against the
  /// expected sequence; `None` indicates no sync is pending.
  pub(crate) pending_sync: Option<AsyncSeq>,
}

const PENDING_AUTOCONNECT_MAX_SYNCS: u8 = 3;

impl State {
  pub(crate) fn update_default_audio_sink(&mut self, name: Option<String>) {
    let Some(name) = name else { return };
    let used_default_output_names = self.historical_output_sink_names.clone().into_keys();
    for n in used_default_output_names {
      if *n == name { continue; }
      self.historical_output_sink_names.insert(n.clone(), false);
    }
    self.historical_output_sink_names.insert(name, true);
  }

  fn get_default_audio_sink_name(&self) -> Option<&String> {
    let Some(sync) = self.historical_output_sink_names.iter().find(|(_, is_default)| **is_default) else { return None };
    Some(sync.0)
  }

  fn get_non_default_audio_sink_names(&self) -> Vec<&String> {
    self.historical_output_sink_names.iter().filter(|(_, is_default)| !**is_default).map(|(name, _)| name).collect()
  }

  fn try_remove_tracked_link(&mut self, id: &u32) -> bool {
    self.tracked_links.remove(id).is_some()
  }

  fn try_remove_tracked_object(&mut self, id: &u32) -> bool {
    self.tracked_objects.remove(id).is_some()
  }

  fn try_remove_node(&mut self, id: &u32) -> bool {
    self.nodes.remove(&id).is_some()
  }

  fn try_remove_input_port(&mut self, id: &u32) -> bool {
    self.input_ports.remove(&id).is_some()
  }

  pub(crate) fn try_remove(&mut self, id: &u32) -> bool {
        self.try_remove_tracked_link(id)
      | self.try_remove_tracked_object(id)
      | self.try_remove_node(id)
      | self.try_remove_input_port(id)
  }

  /// Searches through tracked nodes for an id by the given name. None if nothing is found.
  fn find_node_by_name(&self, name: &str) -> Option<u32> {
    self
      .nodes
      .iter()
      .find(|(_, n)| n.as_str() == name)
      .map(|(id, _)| *id)
  }

  /// Global id of the node that is currently the default audio sink, if known.
  pub(crate) fn get_default_sink_node(&self) -> Option<u32> {
    let name = self.get_default_audio_sink_name()?;
    self.find_node_by_name(name.as_str())
  }

  pub(crate) fn get_non_default_sink_nodes(&self) -> Vec<u32> {
    let names = self.get_non_default_audio_sink_names();
    names.iter().map(|n| self.find_node_by_name(n)).filter_map(|id| id).collect::<Vec<u32>>()
  }

  pub (crate) fn get_all_historical_sink_nodes(&self) -> Vec<u32> {
    let mut v = self.get_non_default_sink_nodes();
    v.push(self.get_default_sink_node().unwrap());
    v
  }

  fn get_all_input_ports_by_node_id(&self, id: &u32) -> Vec<(&u32, &InputPort)> {
    self
      .input_ports
      .iter()
      .filter(|(_, p)| p.node == id.clone())
      .collect()
  }

  /// Returns the input ports of the default sink node as `(port_id, channel)` pairs.
  ///
  /// The default sink node is resolved via [`Self::get_default_sink_node`]; if no
  /// default sink is available, an empty vector is returned. Otherwise, all input
  /// ports belonging to that node are collected and sorted in ascending order by
  /// port id (and then by channel name for equal ids).
  ///
  /// # Returns
  ///
  /// A sorted `Vec` of tuples where the first element is the port id and the
  fn get_default_sink_input_ports(&self) -> Vec<(u32, String)> {
    let Some(sink) = self.get_default_sink_node() else { return Vec::new() };
    let mut ports: Vec<(u32, String)> = self.get_all_input_ports_by_node_id(&sink)
      .iter()
      .map(|(id, p)| (**id, p.channel.clone()))
      .collect();
    ports.sort_unstable();
    ports
  }

  fn port_being_connected(&self, id: &u32) -> bool {
    self.pending_autoconnect.contains_key(id)
  }

  /// Resolves the far side of every link attached to `port_id`.
  ///
  /// For each link in `state` that is still tracked, returns `(node_id, port_id)`
  /// of the endpoint that is *not* `port_id`. Links that have vanished from
  /// `tracked_links` are skipped. Returns an empty vector when `state` is
  /// [`ConnectionState::Disconnected`].
  fn connected_peers(&self, port_id: u32, state: &ConnectionState) -> Vec<(u32, u32)> {
    let ConnectionState::Connected(links) = state else { return Vec::new() };
    links
      .iter()
      .filter_map(|link_id| self.tracked_links.get(link_id))
      .map(|l| {
        let e = l.endpoints;
        if e.output_port == port_id {
          (e.input_node, e.input_port)
        } else {
          (e.output_node, e.output_port)
        }
      })
      .collect()
  }

  /// Returns the properties of the tracked object with the given `id`, but only
  /// if that object is a port whose direction is "out".
  ///
  /// Lookup proceeds in three steps, any of which may short-circuit to `None`:
  /// 1. the object must exist in `tracked_objects`,
  /// 2. the object must have a populated `props` field, and
  /// 3. the properties must describe an output port (see [`port_is_output`]).
  ///
  /// # Arguments
  ///
  /// * `id` - Identifier of the tracked object (e.g. a PipeWire global id).
  ///
  /// # Returns
  ///
  /// `Some(&PropertiesBox)` borrowing the object's properties if it is an
  /// output port, otherwise `None`.
  fn get_output_port_props(&self, id: &u32) -> Option<&PropertiesBox> {
    let Some(obj) = self.tracked_objects.get(&id) else { return None };
    let Some(props) = obj.props.as_ref() else { return None };
    if !port_is_output(props) { return None }
    Some(props)
  }

  pub(crate) fn pull_back_from_node(&mut self, registry: &Registry, id: &u32) {
    let default_nodes = self.get_all_historical_sink_nodes();
    if default_nodes.is_empty() { return };

    self.disconnect_nodes(registry, id, default_nodes);
  }

  fn disconnect_nodes(&mut self, registry: &Registry, id: &u32, default_nodes: Vec<u32>) {
    let to_remove = self
      .tracked_links
      .iter()
      .filter(|(_, link)| {
        peer_node_of(&link.endpoints, *id)
          .map_or(false, |peer| default_nodes.contains(&peer))
      })
      .map(|(link_id, _)| *link_id)
      .collect::<Vec<u32>>();

    for link_id in to_remove {
      match registry.destroy_global(link_id).into_result() {
        Ok(_) => println!("object {id}: delinking {link_id} from previous or current default output"),
        Err(e) => eprintln!("object {id}: failed to delink {link_id}: {e}"),
      }
    }
  }

  pub(crate) fn disconnect_all_non_default_historical_sinks(&mut self, registry: &Registry, id: &u32) {
    let default_nodes = self.get_non_default_sink_nodes();
    if default_nodes.is_empty() { return };

    self.disconnect_nodes(registry, id, default_nodes)
  }

  /// Automatically links every disconnected audio output port to the default sink.
  ///
  /// Scans all tracked output ports whose connection state is currently
  /// [`ConnectionState::Disconnected`] and, for each one, creates one or more
  /// PipeWire links to the matching input port(s) of the default sink node.
  pub(crate) fn autoconnect_disconnected(&mut self, core: &Core, registry: &Registry) {
    let Some(sink_node) = self.get_default_sink_node() else { return };
    let sink_ports = self.get_default_sink_input_ports();
    if sink_ports.is_empty() { return };

    let mut new_links = Vec::new();

    for (port_id, state) in self.current_states() {
      if self.port_being_connected(&port_id) { continue };

      if state != ConnectionState::Disconnected {
        let non_default_sinks = self.get_non_default_sink_nodes();
        let mut all_are_actually_default = true;
        let mut any_are_actually_default = false;
        let mut abort = false;
        for (peer_node, _) in self.connected_peers(port_id, &state) {
          if peer_node == sink_node {
            any_are_actually_default = true;
            continue;
          }

          all_are_actually_default = false;
          if !non_default_sinks.contains(&peer_node) {
            // stand down if the program is connected to something else. this should prevent issues with the frame's spatialization chain connecting to applications after the program patches them to the output, thus causing double audio.
            abort = true;
            break;
          }
        }
        if abort {
          println!("aborting");
          self.pull_back_from_node(registry, &port_id);
          continue
        };

        // at this point, its confirmed all sinks the port is attached to are known historical outputs
        if !all_are_actually_default {
          self.disconnect_all_non_default_historical_sinks(registry, &port_id);
        }

        if any_are_actually_default { continue };
      }

      let Some(props) = self.get_output_port_props(&port_id) else { continue };

      let node = get_id_from_props(props);
      let (Some(node), Some(channel)) = (node, props.get("audio.channel")) else { continue };
      if node == sink_node { continue };

      let channel = channel.to_string();


      for target in solve_for_channel_connection_target(&channel, &sink_ports) {
        let link_props = properties! {
          "link.output.node" => node.to_string(),
          "link.output.port" => port_id.to_string(),
          "link.input.node"  => sink_node.to_string(),
          "link.input.port"  => target.to_string(),
          "object.linger"    => "true",
        };
        match core.create_object::<Link>("link-factory", &link_props) {
          Ok(link) => {
            println!(
              "object {port_id}: auto-linking node {node}:port {port_id} ({channel}) -> sink node {sink_node}:port {target}"
            );
            new_links.push(link);
            self.pending_autoconnect.insert(port_id, 0);
          }
          Err(e) => eprintln!("object {port_id}: failed to create link to port {target}: {e}"),
        }
      }
    }

    self.auto_links.extend(new_links);
  }

  /// Computes the current connection state of every tracked object.
  ///
  /// For each tracked object, this collects the IDs of all tracked links that
  /// have the object as one of their endpoints. Links whose endpoints reference
  /// objects that are not tracked are ignored for those endpoints.
  ///
  /// Returns a map from each tracked object's ID to its [`ConnectionState`]:
  /// - [`ConnectionState::Connected`] with a sorted list of link IDs if the
  ///   object participates in at least one tracked link.
  /// - [`ConnectionState::Disconnected`] if no tracked link references the
  ///   object.
  ///
  fn current_states(&self) -> HashMap<u32, ConnectionState> {
    let mut links_by_object: HashMap<u32, Vec<u32>> = HashMap::new();
    for (link_id, link) in &self.tracked_links {
      for id in link.endpoints.ids() {
        if self.tracked_objects.contains_key(&id) {
          links_by_object.entry(id).or_default().push(*link_id);
        }
      }
    }

    self
      .tracked_objects
      .keys()
      .map(|id| {
        let state = match links_by_object.remove(id) {
          Some(mut links) => {
            links.sort_unstable();
            ConnectionState::Connected(links)
          }
          None => ConnectionState::Disconnected,
        };
        (*id, state)
      })
      .collect()
  }

  /// Prints the connection-state changes that occurred since the last call.
  ///
  /// The current state of every tracked object is computed via
  /// [`Self::current_states`] and compared against `self.last_reported`:
  ///
  /// * Objects that were previously reported but no longer exist are printed
  ///   as `removed` and dropped from `last_reported`.
  pub(crate) fn report_changes(&mut self) {
    let current = self.current_states();

    // Age every pending request; drop the ones the server never answered so the
    // port can be retried instead of being skipped forever.
    self.pending_autoconnect.retain(|id, age| {
      *age += 1;
      if *age > PENDING_AUTOCONNECT_MAX_SYNCS {
        println!("object {id}: autoconnect request expired, will retry");
        false
      } else {
        true
      }
    });

    let gone: Vec<u32> = self
      .last_reported
      .keys()
      .filter(|id| !current.contains_key(id))
      .copied()
      .collect();
    for id in gone {
      println!("object {id}: removed");
      self.last_reported.remove(&id);
      self.pending_autoconnect.remove(&id);
    }

    for (id, state) in current {
      if let ConnectionState::Connected(_) = &state {
        // The requested link has shown up (or something else connected it).
        self.pending_autoconnect.remove(&id);
      }
      if self.last_reported.get(&id) == Some(&state) {
        continue;
      }
      if self.last_reported.get(&id) == Some(&state) {
        continue;
      }
      match &state {
        ConnectionState::Disconnected => println!("object {id}: NOT CONNECTED"),
        ConnectionState::Connected(links) => {
          for link_id in links {
            if let Some(l) = self.tracked_links.get(link_id) {
              let e = l.endpoints;
              println!(
                "object {id}: connected via link {link_id} (node {}:port {} -> node {}:port {})",
                e.output_node, e.output_port, e.input_node, e.input_port
              );
            }
          }
        }
      }
      self.last_reported.insert(id, state);
    }
  }
}

/// Returns `true` if the port described by `props` is an output port.
///
/// This checks the `port.direction` property and compares it against the
/// string `"out"`. Any other value, or a missing property, is treated as
/// not being an output port.
///
/// # Arguments
///
/// * `props` - The property set of a port (e.g. from a PipeWire port object).
///
/// # Returns
///
/// `true` if `port.direction` equals `"out"`, `false` otherwise.
fn port_is_output(props: &PropertiesBox) -> bool {
  props.get("port.direction") == Some("out")
}

/// Extracts the node ID from a `PropertiesBox`.
///
/// Looks up the `"node.id"` property and attempts to parse its value as a `u32`.
///
fn get_id_from_props(props: &PropertiesBox) -> Option<u32> {
  props.get("node.id").and_then(|n| n.parse::<u32>().ok())
}

/// Selects the sink port IDs that a source channel should be connected to.
///
/// Ports whose channel name exactly matches `channel` are preferred. If no
/// such port exists, every port in `sink_ports` is returned as a fallback,
/// so the source channel is still routed somewhere rather than being dropped.
///
/// # Arguments
///
/// * `channel` - The channel name of the source port (e.g. `"FL"`, `"FR"`, `"MONO"`).
/// * `sink_ports` - The sink's available ports as `(port_id, channel_name)` pairs.
///
/// # Returns
///
/// A list of sink port IDs. This is either the subset with a matching channel
/// name, or all sink port IDs when no exact match is found. The result is empty
/// only if `sink_ports` is empty.
fn solve_for_channel_connection_target(channel: &str, sink_ports: &[(u32, String)]) -> Vec<u32> {
  let exact: Vec<u32> = sink_ports
    .iter()
    .filter(|(_, c)| c == channel)
    .map(|(id, _)| *id)
    .collect();
  if !exact.is_empty() {
    return exact;
  }
  sink_ports.iter().map(|(id, _)| *id).collect()
}

/// Returns the node on the far side of `endpoints` relative to `port_id`,
/// or `None` if `port_id` is not one of the link's ports.
fn peer_node_of(endpoints: &LinkEndpoints, port_id: u32) -> Option<u32> {
  if endpoints.output_port == port_id {
    Some(endpoints.input_node)
  } else if endpoints.input_port == port_id {
    Some(endpoints.output_node)
  } else {
    None
  }
}