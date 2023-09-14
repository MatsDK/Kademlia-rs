import { writable } from "svelte/store";
import { type NodeInfo } from "./bindings";

export const nodes = writable<NodeInfo[]>([]);
export const bootstrap_nodes = writable<Array<[string, string]>>([]);
// Map containging each node's latest response on a GET_RECORD call containing: (key, publisher, value)
export const last_get_records = writable<Map<string, [string, string, string]>>(new Map());
