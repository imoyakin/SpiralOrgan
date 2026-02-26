use crate::traits::{ChannelAdapter, CoreResult, NotificationSink};

pub struct GatewayService<C, N> {
    channel: C,
    notifications: N,
}

impl<C, N> GatewayService<C, N> {
    pub fn new(channel: C, notifications: N) -> Self {
        Self {
            channel,
            notifications,
        }
    }
}

impl<C, N> GatewayService<C, N>
where
    C: ChannelAdapter + Send + Sync,
    N: NotificationSink + Send + Sync,
{
    pub fn send_message(&self, to: &str, content: &str) -> CoreResult<()> {
        self.channel.send(to, content)
    }

    pub fn notify_idle(&self, session_id: &str) -> CoreResult<()> {
        self.notifications.notify_idle_waiting(session_id)
    }
}
