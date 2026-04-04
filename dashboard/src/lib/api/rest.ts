import { fetchRest } from "./client";
import type {
  NodeInfo,
  MempoolInfo,
  PeersResponse,
  LightningInfo,
  LightningChannel,
  LightningPayment,
} from "./types";

export const getNodeInfo = () => fetchRest<NodeInfo>("/api/info");
export const getMempoolInfo = () => fetchRest<MempoolInfo>("/api/mempool");
export const getPeers = () => fetchRest<PeersResponse>("/api/peers");
export const getLightningInfo = () =>
  fetchRest<LightningInfo>("/api/lightning/info");
export const getLightningChannels = () =>
  fetchRest<{ channels: LightningChannel[] }>("/api/lightning/channels");
export const getLightningPayments = () =>
  fetchRest<{ payments: LightningPayment[] }>("/api/lightning/payments");
