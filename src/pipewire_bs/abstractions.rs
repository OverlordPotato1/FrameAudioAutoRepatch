use crate::pipewire_bs::link::LinkEndpoints;
use crate::pipewire_bs::state::State;
use pipewire::link::{Link, LinkListener};
use pipewire::registry::{GlobalObject, RegistryRc};
use pipewire::spa::utils::dict::DictRef;
use std::cell::RefCell;
use std::rc::Rc;

pub fn is_not_default(props: &DictRef) -> bool { props.get("metadata.name") != Some("default") }
pub fn endpoint_not_tracked(endpoints: LinkEndpoints, state_add: &Rc<RefCell<State>>) -> bool { !endpoints.touches(&state_add.borrow().tracked_objects) }

/// Binds a global object as a [`Link`] using the given registry.
///
/// Attempts to bind `global` through `registry_bind`. On success, the resulting
/// [`Link`] is returned. If binding fails, an error message including the
/// global's ID is printed to stderr and `None` is returned.
///
pub fn bind_link(registry_bind: &RegistryRc, global: &GlobalObject<&DictRef>) -> Option<Link> {
  Some(match registry_bind.bind(global) {
    Ok(link) => link,
    Err(e) => {
      eprintln!("failed to bind link {}: {e}", global.id);
      return None;
    }
  })
}

/// Creates a listener for a single PipeWire link that logs its state changes.
///
/// Registers a local listener on the given `link` whose `info` callback
pub fn create_individual_link_listener(global: &GlobalObject<&DictRef>, link: &Link) -> LinkListener {
  let link_id = global.id;
  let listener = link
    .add_listener_local()
    .info(move |info| println!("link {link_id} state: {:?}", info.state()))
    .register();
  listener
}

pub fn port_is_input(props: &DictRef) -> bool { props.get("port.direction") == Some("in") }

pub fn get_port_node_id(props: &DictRef) -> Option<u32> { props.get("node.id").and_then(|n| n.parse::<u32>().ok()) }