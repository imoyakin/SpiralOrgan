use super::CoreResult;

pub trait ChannelAdapter: Send + Sync {
    fn channel_id(&self) -> &str;
    fn send(&self, to: &str, content: &str) -> CoreResult<()>;
}

pub trait NotificationSink: Send + Sync {
    fn notify_done(&self, session_id: &str, summary: &str) -> CoreResult<()>;
    fn notify_idle_waiting(&self, session_id: &str) -> CoreResult<()>;
}
