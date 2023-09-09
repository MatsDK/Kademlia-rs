<script lang="ts">
    import type { NodeInfo } from "../lib/bindings";
    import { taurpc } from "../lib/ipc";
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

    const getRecord = async (e: SubmitEvent) => {};

    const putRecord = async (e: SubmitEvent) => {
        e.preventDefault();

        try {
            await taurpc.put_record(node.key, key.trim() || null, value);
            tab = "Records";
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
        <div class="flex gap-3 px-2 text-sm">
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
        {#if tab === "Records"}
            {#each node.records ?? [] as [key, publisher, value]}
                {@const is_publisher = publisher == node.key}
                <div>
                    <span>{key}</span>
                    <span>{value}</span>
                    {#if is_publisher}
                        <span
                            class="bg-gray-300 text-black text-xs px-1 rounded-md font-semibold"
                            >Publisher</span
                        >
                    {/if}
                </div>
            {/each}
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
        <RoutingTable buckets={node.buckets} />
    </div>
</div>
