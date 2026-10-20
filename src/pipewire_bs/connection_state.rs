/// Represents the current connection status of a client.
///
/// A connection is either fully disconnected or connected with a set of
/// active channel/stream identifiers.
///
/// # Variants
///
/// * `Disconnected` - No active connection exists. No channels are open.
/// * `Connected(Vec<u32>)` - A connection is established. The contained
///   vector holds the identifiers of all currently open channels (may be
///   empty if connected but no channels have been opened yet).
///
/// # Examples
///
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
  Disconnected,
  Connected(Vec<u32>),
}