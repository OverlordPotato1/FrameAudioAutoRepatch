use crate::pipewire_bs::connection_state::ConnectionState;
use crate::pipewire_bs::input_port::InputPort;
use crate::pipewire_bs::tracked_link::TrackedLink;
use pipewire::__properties__ as properties;
use pipewire::core::Core;
use pipewire::link::Link;
use pipewire::metadata::{Metadata, MetadataListener};
use pipewire::properties::PropertiesBox;
use pipewire::registry::GlobalObject;
use pipewire::spa::utils::result::AsyncSeq;
use std::collections::{HashMap, HashSet};

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
  /// `node.name` of the current default audio sink (from the "default" metadata).
  pub(crate) default_sink_name: Option<String>,
  /// Proxy + listener for the "default" metadata object; kept alive to receive updates.
  pub(crate) default_metadata: Option<(Metadata, MetadataListener)>,
  /// Links we created ourselves; kept alive so the proxies are not destroyed.
  auto_links: Vec<Link>,
  /// Ports for which a link has been requested but not yet announced by the server.
  pending_autoconnect: HashSet<u32>,
  /// Sequence number of the sync operation currently in flight, if any.
  /// Sequence number of the sync operation currently in flight, if any.
  ///
  /// Set when an asynchronous sync is initiated and cleared once that
  /// operation completes. While this is `Some`, the value identifies the
  /// outstanding request so its completion can be matched against the
  /// expected sequence; `None` indicates no sync is pending.
  pub(crate) pending_sync: Option<AsyncSeq>,
}

impl State {
  pub(crate) fn try_remove(&mut self, id: &u32) -> bool {
    self.tracked_links.remove(&id).is_some()
      | self.tracked_objects.remove(&id).is_some()
      | self.nodes.remove(&id).is_some()
      | self.input_ports.remove(&id).is_some()
  }
  
  /// Global id of the node that is currently the default audio sink, if known.
  pub(crate) fn default_sink_node(&self) -> Option<u32> {
    let name = self.default_sink_name.as_deref()?;
    self
      .nodes
      .iter()
      .find(|(_, n)| n.as_str() == name)
      .map(|(id, _)| *id)
  }

  /// `(port id, channel)` of every audio input port on the default sink.
  fn sink_input_ports(&self) -> Vec<(u32, String)> {
    let Some(sink) = self.default_sink_node() else { return Vec::new() };
    let mut ports: Vec<(u32, String)> = self
      .input_ports
      .iter()
      .filter(|(_, p)| p.node == sink)
      .map(|(id, p)| (*id, p.channel.clone()))
      .collect();
    ports.sort_unstable();
    ports
  }

  /// Sink ports an output port with `channel` should be linked to:
  /// same channel if the sink has it, otherwise (e.g. MONO) every sink input.
  fn targets_for(channel: &str, sink_ports: &[(u32, String)]) -> Vec<u32> {
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

  /// Links every tracked *output* port that currently has no connection
  /// to the default audio sink.
  pub(crate) fn autoconnect_disconnected(&mut self, core: &Core) {
    let Some(sink_node) = self.default_sink_node() else { return };
    let sink_ports = self.sink_input_ports();
    if sink_ports.is_empty() {
      return;
    }

    let current = self.current_states();
    let mut new_links = Vec::new();

    for (port_id, state) in current {
      if state != ConnectionState::Disconnected || self.pending_autoconnect.contains(&port_id) {
        continue;
      }
      let Some(obj) = self.tracked_objects.get(&port_id) else { continue };
      let Some(props) = obj.props.as_ref() else { continue };
      if props.get("port.direction") != Some("out") {
        continue;
      }
      let node = props.get("node.id").and_then(|n| n.parse::<u32>().ok());
      let (Some(node), Some(channel)) = (node, props.get("audio.channel")) else { continue };
      if node == sink_node {
        continue;
      }

      for target in Self::targets_for(channel, &sink_ports) {
        let link_props = properties! {
          "link.output.node" => node.to_string(),
          "link.output.port" => port_id.to_string(),
          "link.input.node"  => sink_node.to_string(),
          "link.input.port"  => target.to_string(),
          // Keep the link alive on the server even if our proxy goes away.
          "object.linger"    => "true",
        };
        match core.create_object::<Link>("link-factory", &link_props) {
          Ok(link) => {
            println!(
              "object {port_id}: auto-linking node {node}:port {port_id} ({channel}) -> sink node {sink_node}:port {target}"
            );
            new_links.push(link);
            self.pending_autoconnect.insert(port_id);
          }
          Err(e) => eprintln!("object {port_id}: failed to create link to port {target}: {e}"),
        }
      }
    }

    self.auto_links.extend(new_links);
  }
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