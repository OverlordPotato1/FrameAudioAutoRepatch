mod pipewire_bs;

use crate::pipewire_bs::utils::{create_core_listener, create_sync_callback};
use pipewire as pw;
use pipewire::context::ContextRc;
use pipewire::core::CoreRc;
use pipewire::main_loop::MainLoopRc;
use pipewire::types::ObjectType;
use pipewire_bs::state::State;
use pipewire_bs::utils;
use std::cell::RefCell;
use std::rc::Rc;

// Filters
const INSTANT_REJECT_NAME_FILTER: [&str; 2] = ["monitor_", "capture_"];
const INSTANT_REJECT_FORMAT_FILTER: [&str; 1] = ["midi"];
const INSTANT_REJECT_CHANNEL_FILTER: [&str; 7] = ["RL", "RR", "LFE", "FC", "LFE", "SL", "SR"];
const INSTANT_REJECT_PATH_FILTER: [&str; 2] = ["sunshine", "sink-sunshine"];
const INVISIBLE_SINKS: [&str; 3] = ["vesktop", "fox", "recorder"]; // pretend these sinks do not exist in all conditions

fn main() -> Result<(), Box<dyn std::error::Error>> {
  pw::init();

  let mainloop = MainLoopRc::new(None)?;
  let context = ContextRc::new(&mainloop, None)?;
  let core: &'static CoreRc = Box::leak(Box::new(context.connect_rc(None)?));
  let registry = core.get_registry_rc()?;

  let state: &'static Rc<RefCell<State>> = Box::leak(Box::new(Rc::new(RefCell::new(State::default()))));

  let request_sync = Rc::new(create_sync_callback(core, state));
  let _core_listener = create_core_listener(core, &registry, state);

  let _listener = {
    let state_add = state.clone();
    let state_remove = state.clone();
    let registry_bind = registry.clone();
    let sync_add = request_sync.clone();
    let sync_remove = request_sync.clone();

    registry
      .add_listener_local()
      .global(move |global| {
        let Some(props) = global.props else { return };

        match &global.type_ {
          ObjectType::Metadata => if utils::handle_metadata(&state_add, &registry_bind, &sync_add, global, props) { return; }
          ObjectType::Node => { utils::handle_node(&state_add, &sync_add, global, props) }
          ObjectType::Link => if utils::handle_link(&state_add, &registry_bind, &sync_add, global, props) { return; }
          ObjectType::Port => { utils::handle_port(&state_add, &sync_add, global, props) }
          _ => {}
        }
      })
      .global_remove(move |id| {
        let mut s = state_remove.borrow_mut();
        let changed = s.try_remove(&id);
        drop(s);
        if changed {
          sync_remove();
        }
      })
      .register()
  };

  println!("Hello, world!");

  mainloop.run();

  Ok(())
}

