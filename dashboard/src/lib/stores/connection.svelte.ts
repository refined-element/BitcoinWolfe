import type { ConnectionConfig } from "../api/types";

const STORAGE_KEY = "bitcoinwolfe_connection";

const defaults: ConnectionConfig = {
  url: "",
  user: "",
  password: "",
  pollInterval: 3000,
};

function load(): ConnectionConfig {
  if (typeof localStorage === "undefined") return { ...defaults };
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return { ...defaults, ...JSON.parse(raw) };
  } catch {
    /* ignore */
  }
  return { ...defaults };
}

let connection = $state<ConnectionConfig>(load());

export function getConnection(): ConnectionConfig {
  return connection;
}

export function setConnection(config: Partial<ConnectionConfig>) {
  connection = { ...connection, ...config };
  if (typeof localStorage !== "undefined") {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(connection));
  }
}

export function connectionStore() {
  return {
    get current() {
      return connection;
    },
    set(config: Partial<ConnectionConfig>) {
      setConnection(config);
    },
  };
}
