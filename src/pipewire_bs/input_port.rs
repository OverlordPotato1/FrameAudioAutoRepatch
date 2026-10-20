/// An audio input port that belongs to some node; candidate target for auto-linking.
#[derive(Debug, Clone)]
pub struct InputPort {
  pub node: u32,
  pub channel: String,
}