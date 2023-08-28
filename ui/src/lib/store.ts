import { writable } from "svelte/store";
import { type NodeInfo } from "./bindings";

export const nodes = writable<NodeInfo[]>([]);
export const bootstrap_nodes = writable<Array<[string, string]>>([]);
