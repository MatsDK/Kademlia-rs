<script lang="ts">
    import type { NodeInfo } from "../lib/bindings";
    import { taurpc } from "../lib/ipc";
    import { nodes } from "../lib/store";
    import RoutingTable from "./RoutingTable.svelte";

    export let node: NodeInfo;

    const tabs = ["Records", "Get", "Put"] as const;
    type Tabs = (typeof tabs)[number];

    let tab: Tabs = "Records";

    function cx(...cns: (boolean | string | undefined)[]): string {
        return cns.filter(Boolean).join(" ");
    }

    let key = "";
    let value = "";

    const getRecord = async (e: SubmitEvent) => {
        e.preventDefault();

        try {
            await taurpc.get_record(node.key, key.trim());
            tab = "Records";
            key = "";
        } catch (error) {
            console.error(error);
        }
    };

    const putRecord = async (e: SubmitEvent) => {
        e.preventDefault();

        try {
            await taurpc.put_record(node.key, key.trim() || null, value);
            tab = "Records";
            key = "";
            value = "";
        } catch (error) {
            console.error(error);
        }
    };

    const removeRecord = async (record_key: string) => {
        try {
            await taurpc.remove_record(node.key, record_key);
        } catch (error) {
            console.error(error);
        }
    };

    const stopNode = async () => {
        try {
            await taurpc.close_node(node.key);
            $nodes = $nodes.filter((n) => n.key != node.key);
        } catch (error) {
            console.error(error);
        }
    };
</script>

<div class="border border-secondary h-full flex flex-col">
    <div class="flex justify-between items-center px-2">
        <span class="font-semibold">
            {node.key}
            {#if node.is_bootstrap}
                <span
                    class="text-xs bg-gray-300 text-black rounded-md px-2 font-semibold py-px whitespace-nowrap"
                    >Bootstrap Node</span
                >
            {/if}
        </span>
        <span class="text-secondary-text text-sm">
            {node.addr}
        </span>
    </div>
    <div class="flex-1 flex flex-col overflow-auto">
        <div class="flex justify-between px-2 text-sm">
            <div class="flex gap-3">
                {#each tabs as label}
                    <button
                        class={cx(
                            " hover:opacity-100 transition-opacity",
                            label === tab
                                ? "opacity-100 font-semibold"
                                : "opacity-60"
                        )}
                        on:click={() => (tab = label)}>{label}</button
                    >
                {/each}
            </div>
            <button
                on:click={stopNode}
                class="opacity-60 hover:opacity-100 transition-opacity"
                >Stop</button
            >
        </div>
        {#if tab === "Records"}
            <div class="px-2 my-1 grid grid-cols-2 overflow-hidden gap-1">
                {#each node.records ?? [] as [key, publisher, value]}
                    {@const is_publisher = publisher == node.key}
                    <div
                        class="group border rounded-sm border-secondary px-2 overflow-hidden text-sm"
                    >
                        <div class="max-w-full truncate font-semibold">
                            {key}
                        </div>
                        <pre
                            class="truncate max-w-full text-secondary-text-lighter">

                            {value}
                    </pre>
                        {#if is_publisher}
                            <div
                                class="flex justify-end gap-1 items-center pb-px"
                            >
                                <button
                                    class="opacity-0 group-hover:opacity-60 transition-opacity text-xs"
                                    on:click={() => removeRecord(key)}
                                    >Remove</button
                                >
                                <span
                                    class="bg-gray-300 text-black text-xs px-1 rounded-md font-semibold h-[16px]"
                                    >Publisher</span
                                >
                            </div>
                        {/if}
                    </div>
                {/each}
            </div>
        {:else if tab === "Get"}
            <div
                class="w-full flex-1 flex flex-col justify-center items-center h-full"
            >
                <form
                    class="flex flex-col min-w-[250px] gap-2"
                    on:submit={getRecord}
                >
                    <h1 class="font-semibold">Get record</h1>
                    <label for="key" class="opacity-60 text-sm">Key</label>
                    <input
                        bind:value={key}
                        id="key"
                        type="text"
                        class="bg-primary border border-secondary rounded-md text-sm px-1"
                    />

                    <button
                        type="submit"
                        class="bg-gray-300 hover:bg-white text-black transition-colors rounded-md px-2"
                        >Get record</button
                    >
                </form>
            </div>
        {:else if tab === "Put"}
            <div
                class="w-full flex flex-col justify-center items-center h-full"
            >
                <form
                    class="flex flex-col min-w-[250px] gap-2"
                    on:submit={putRecord}
                >
                    <h1 class="font-semibold">Put record</h1>
                    <label for="value" class="opacity-60 text-sm">Value</label>
                    <textarea
                        id="value"
                        bind:value
                        class="bg-primary border border-secondary rounded-md text-sm px-1"
                    />
                    <label for="key" class="opacity-60 text-sm"
                        >Key (optional)</label
                    >
                    <input
                        class="bg-primary border border-secondary rounded-md text-sm px-1"
                        bind:value={key}
                        id="key"
                        type="text"
                    />
                    <button
                        type="submit"
                        class="bg-gray-300 hover:bg-white text-black transition-colors rounded-md px-2"
                        >Put record</button
                    >
                </form>
            </div>
        {/if}
    </div>
    <div class="h-[30%] border-t border-secondary relative">
        <span
            class="absolute top-[-11px] left-2 bg-primary px-2 text-sm text-secondary-text z-10"
            >Routing table</span
        >
        <RoutingTable buckets={node.buckets} node_id={node.key} />
    </div>
</div>
