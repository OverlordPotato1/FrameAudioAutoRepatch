use crate::pipewire_bs::link::LinkEndpoints;
use pipewire::link::{Link, LinkListener};

/// A [`Link`] whose lifecycle is observed and whose endpoints are tracked.
///
/// `TrackedLink` bundles together a proxy to the underlying link, a listener
/// that receives state-change notifications for it, and the resolved
pub struct TrackedLink {
  pub endpoints: LinkEndpoints,
  _proxy: Link,
  _listener: LinkListener,
}

impl TrackedLink {
  pub(crate) fn new(endpoints: LinkEndpoints, proxy: Link, listener: LinkListener) -> Self {
    Self {
      endpoints,
      _proxy: proxy,
      _listener: listener
    }
  }
  
  pub(crate) fn into_proxy(self) -> Link {
    self._proxy
  }
}