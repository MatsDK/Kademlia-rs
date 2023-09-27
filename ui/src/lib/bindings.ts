// This file has been generated by Specta. DO NOT EDIT.

export type TauRpcApiInputTypes = { proc_name: "new_node"; input_type: null } | { proc_name: "add_bootstrap_node"; input_type: { __taurpc_type: string } } | { proc_name: "remove_bootstrap_node"; input_type: { __taurpc_type: string } } | { proc_name: "disconnect_peer"; input_type: [string, string] } | { proc_name: "close_node"; input_type: { __taurpc_type: string } } | { proc_name: "get_record"; input_type: [string, string] } | { proc_name: "put_record"; input_type: [string, string | null, string] } | { proc_name: "remove_record"; input_type: [string, string] } | { proc_name: "bootstrap_nodes_changed"; input_type: { __taurpc_type: ([string, string])[] } } | { proc_name: "routing_table_changed"; input_type: { __taurpc_type: RoutingTableChanged } } | { proc_name: "record_store_changed"; input_type: { __taurpc_type: RecordStoreChanged } }

export type RecordStoreChanged = { node_key: string; records: ([string, string, string])[] }

export type TauRpcApiOutputTypes = { proc_name: "new_node"; output_type: NodeInfo } | { proc_name: "add_bootstrap_node"; output_type: null } | { proc_name: "remove_bootstrap_node"; output_type: null } | { proc_name: "disconnect_peer"; output_type: null } | { proc_name: "close_node"; output_type: null } | { proc_name: "get_record"; output_type: null } | { proc_name: "put_record"; output_type: null } | { proc_name: "remove_record"; output_type: null } | { proc_name: "bootstrap_nodes_changed"; output_type: null } | { proc_name: "routing_table_changed"; output_type: null } | { proc_name: "record_store_changed"; output_type: null }

export type RoutingTableChanged = { node_key: string; buckets: { [key: number]: ([string, string, string])[] } }

export type NodeInfo = { key: string; addr: string; is_bootstrap: boolean; buckets: { [key: number]: ([string, string, string])[] }; records: ([string, string, string])[] }

const ARGS_MAP = {"":"{\"close_node\":[\"node_id\"],\"get_record\":[\"node_key\",\"record_key\"],\"record_store_changed\":[\"records\"],\"put_record\":[\"node_key\",\"record_key\",\"value\"],\"routing_table_changed\":[\"routing_table\"],\"add_bootstrap_node\":[\"key\"],\"remove_record\":[\"node_key\",\"record_key\"],\"disconnect_peer\":[\"node_id\",\"connect_peer_id\"],\"new_node\":[],\"remove_bootstrap_node\":[\"key\"],\"bootstrap_nodes_changed\":[\"bootstrap_nodes\"]}"}
import { createTauRPCProxy as createProxy } from "taurpc"

export const createTauRPCProxy = () => createProxy<Router>(ARGS_MAP)

type Router = {
	'': [TauRpcApiInputTypes, TauRpcApiOutputTypes],
}