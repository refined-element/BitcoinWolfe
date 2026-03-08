//! Type aliases for LDK's heavily-generic types.
//!
//! LDK uses trait-based generics extensively. These aliases pin concrete types
//! so the rest of the crate doesn't need to spell out 10+ type parameters.

use std::sync::{Arc, Mutex};

use lightning::chain::chainmonitor::ChainMonitor;
use lightning::chain::Filter;
use lightning::ln::channelmanager::ChannelManager;
use lightning::ln::peer_handler::{IgnoringMessageHandler, PeerManager as LdkPeerManager};
use lightning::onion_message::messenger::{DefaultMessageRouter, OnionMessenger};
use lightning::routing::gossip::{NetworkGraph, P2PGossipSync};
use lightning::routing::router::DefaultRouter;
use lightning::routing::scoring::{ProbabilisticScorer, ProbabilisticScoringFeeParameters};
use lightning::routing::utxo::UtxoLookup;
use lightning::sign::KeysManager;
use lightning::util::persist::MonitorUpdatingPersister;

use crate::broadcaster::WolfeBroadcaster;
use crate::fee_estimator::WolfeFeeEstimator;
use crate::logger::WolfeLogger;
use crate::persister::WolfeKVStore;

// ── Core component aliases ──────────────────────────────────────────────

pub type WolfeNetworkGraph = NetworkGraph<Arc<WolfeLogger>>;

pub type WolfeScorer = ProbabilisticScorer<Arc<WolfeNetworkGraph>, Arc<WolfeLogger>>;

pub type WolfeRouter = DefaultRouter<
    Arc<WolfeNetworkGraph>,
    Arc<WolfeLogger>,
    Arc<KeysManager>,
    Arc<Mutex<WolfeScorer>>,
    ProbabilisticScoringFeeParameters,
    WolfeScorer,
>;

pub type WolfeMessageRouter =
    DefaultMessageRouter<Arc<WolfeNetworkGraph>, Arc<WolfeLogger>, Arc<KeysManager>>;

pub type WolfeGossipSync = P2PGossipSync<
    Arc<WolfeNetworkGraph>,
    Arc<dyn UtxoLookup + Send + Sync>,
    Arc<WolfeLogger>,
>;

// MonitorUpdatingPersister<K, L, ES, SP, BI, FE>
pub type WolfeMonitorPersister = MonitorUpdatingPersister<
    Arc<WolfeKVStore>,
    Arc<WolfeLogger>,
    Arc<KeysManager>,       // ES: EntropySource
    Arc<KeysManager>,       // SP: SignerProvider
    Arc<WolfeBroadcaster>,  // BI: BroadcasterInterface
    Arc<WolfeFeeEstimator>, // FE: FeeEstimator
>;

// ChainMonitor<ChannelSigner, C, T, F, L, P, ES>
pub type WolfeChainMonitor = ChainMonitor<
    lightning::sign::InMemorySigner,
    Arc<dyn Filter + Send + Sync>,
    Arc<WolfeBroadcaster>,
    Arc<WolfeFeeEstimator>,
    Arc<WolfeLogger>,
    Arc<WolfeMonitorPersister>,
    Arc<KeysManager>, // ES: EntropySource
>;

// ChannelManager<M, T, ES, NS, SP, F, R, MR, L>
pub type WolfeChannelManager = ChannelManager<
    Arc<WolfeChainMonitor>,
    Arc<WolfeBroadcaster>,
    Arc<KeysManager>, // ES: EntropySource
    Arc<KeysManager>, // NS: NodeSigner
    Arc<KeysManager>, // SP: SignerProvider
    Arc<WolfeFeeEstimator>,
    Arc<WolfeRouter>,
    Arc<WolfeMessageRouter>,
    Arc<WolfeLogger>,
>;

// OnionMessenger<ES, NS, L, NL, MR, OMH, APH, DRH, CMH>
pub type WolfeOnionMessenger = OnionMessenger<
    Arc<KeysManager>,         // ES: EntropySource
    Arc<KeysManager>,         // NS: NodeSigner
    Arc<WolfeLogger>,         // L: Logger
    Arc<WolfeChannelManager>, // NL: NodeIdLookUp
    Arc<WolfeMessageRouter>,  // MR: MessageRouter
    Arc<WolfeChannelManager>, // OMH: OffersMessageHandler
    IgnoringMessageHandler,   // APH: AsyncPaymentsMessageHandler
    IgnoringMessageHandler,   // DRH: DNSResolverMessageHandler
    IgnoringMessageHandler,   // CMH: CustomOnionMessageHandler
>;

// PeerManager<Descriptor, CM, RM, OM, L, CMH, NS, SM>
pub type WolfePeerManager = LdkPeerManager<
    lightning_net_tokio::SocketDescriptor,
    Arc<WolfeChannelManager>, // CM: ChannelMessageHandler
    Arc<WolfeGossipSync>,     // RM: RoutingMessageHandler
    Arc<WolfeOnionMessenger>, // OM: OnionMessageHandler
    Arc<WolfeLogger>,         // L: Logger
    IgnoringMessageHandler,   // CMH: CustomMessageHandler
    Arc<KeysManager>,         // NS: NodeSigner
    Arc<WolfeChainMonitor>,   // SM: SendOnlyMessageHandler
>;
