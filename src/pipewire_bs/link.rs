use pipewire::properties::PropertiesBox;
use pipewire::registry::GlobalObject;
use pipewire::spa::utils::dict::DictRef;
use std::collections::HashMap;

/// Describes the two endpoints of a link between nodes in a graph.
///
/// A link connects an output port on a source node to an input port on a
/// destination node. Each endpoint is identified by the ID of the node it
/// belongs to together with the index (or ID) of the port on that node.
///
/// This type is `Copy`, so it can be passed around cheaply when wiring
/// or inspecting graph connections.
///
#[derive(Debug, Clone, Copy)]
pub struct LinkEndpoints {
  pub(crate) output_node: u32,
  pub(crate) output_port: u32,
  pub(crate) input_node: u32,
  pub(crate) input_port: u32,
}

impl LinkEndpoints {
  /// Constructs a link from a PipeWire property dictionary.
  ///
  /// Reads the `link.output.node`, `link.output.port`, `link.input.node`,
  /// and `link.input.port` entries from `props` and parses each as a `u32`.
  ///
  /// Returns `None` if any of the required keys is missing or if a value
  /// cannot be parsed as an unsigned integer.
  pub(crate) fn from_props(props: &DictRef) -> Option<Self> {
    let get = |key: &str| props.get(key)?.parse::<u32>().ok();
    Some(Self {
      output_node: get("link.output.node")?,
      output_port: get("link.output.port")?,
      input_node: get("link.input.node")?,
      input_port: get("link.input.port")?,
    })
  }

  /// Returns the four identifiers that define this connection as a fixed-size array.
  ///
  /// The array is ordered as:
  /// `[output_node, output_port, input_node, input_port]`.
  ///
  /// This is useful for hashing, comparison, or serializing the connection
  /// in a compact, positional form.
  pub(crate) fn ids(&self) -> [u32; 4] {
    [self.output_node, self.output_port, self.input_node, self.input_port]
  }

  /// Returns `true` if any of the object IDs referenced by `self` are present
  /// in the given `tracked` map.
  ///
  /// This is useful for quickly determining whether an event or change set is
  /// relevant to the currently tracked global objects, without having to
  /// inspect each ID individually.
  ///
  pub(crate) fn touches(&self, tracked: &HashMap<u32, GlobalObject<PropertiesBox>>) -> bool {
    self.ids().iter().any(|id| tracked.contains_key(id))
  }
}