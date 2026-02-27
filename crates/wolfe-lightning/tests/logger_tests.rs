use std::sync::Arc;

use lightning::util::logger::{Level, Logger, Record};

use wolfe_lightning::logger::WolfeLogger;

#[test]
fn logger_handles_all_levels_without_panic() {
    let logger = WolfeLogger;

    let levels = [
        Level::Gossip,
        Level::Trace,
        Level::Debug,
        Level::Info,
        Level::Warn,
        Level::Error,
    ];

    for level in levels {
        logger.log(Record::new(
            level,
            None, // peer_id: Option<PublicKey>
            None, // channel_id: Option<ChannelId>
            format_args!("test message at {:?}", level),
            "test_module",
            "",
            42,
            None, // payment_hash: Option<PaymentHash>
        ));
    }
}

#[test]
fn logger_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<WolfeLogger>();
}

#[test]
fn logger_works_behind_arc() {
    let logger: Arc<WolfeLogger> = Arc::new(WolfeLogger);
    logger.log(Record::new(
        Level::Info,
        None,
        None,
        format_args!("arc logger test"),
        "test",
        "",
        1,
        None,
    ));
}
