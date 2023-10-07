<script lang="ts">
    import "./index.css";
    import NodesContainer from "./components/NodesContainer.svelte";
    import { taurpc } from "./lib/ipc";
    import { nodes } from "./lib/store";
    import BootstrapNodes from "./components/BootstrapNodes.svelte";
    import { onMount } from "svelte";
    import { cx } from "./components/Node.svelte";

    let genRandomKey = true;
    let keyInput = "";
    let makeBootstrap = false;

    const newNode = async (e: SubmitEvent) => {
        e.preventDefault();
        try {
            const res = await taurpc.new_node(
                !genRandomKey ? keyInput.trim() : null,
                makeBootstrap
            );
            keyInput = "";
            nodes.set([...$nodes, res]);
        } catch (e) {
            console.error("Error creating node", e);
        }
    };

    onMount(() => {
        const unlisten: Array<() => void> = [];
        unlisten.push(
            taurpc.routing_table_changed.on((changed) => {
                let node = $nodes.find(({ key }) => key === changed.node_key);
                if (node) {
                    node.buckets = changed.buckets;
                    nodes.set($nodes);
                }
            })
        );
        unlisten.push(
            taurpc.record_store_changed.on((changed) => {
                let node = $nodes.find(({ key }) => key === changed.node_key);
                if (node) {
                    node.records = changed.records;
                    nodes.set($nodes);
                }
            })
        );
        unlisten.push(
            taurpc.get_record_finished.on((node_key, record) => {
                let node = $nodes.find(({ key }) => key === node_key);
                if (node) {
                    node.last_get_res = record;
                    nodes.set($nodes);
                }
            })
        );

        return unlisten;
    });
</script>

<div
    class="flex flex-col w-screen h-screen bg-primary text-white overflow-hidden"
>
    <NodesContainer />
    <div
        class="border-t min-h-[300px] border-secondary bg-black bg-opacity-30 flex"
    >
        <div class="flex-1">
            <div
                class="w-full border-b border-secondary text-lg font-semibold pl-3"
            >
                Kademlia-rs
            </div>

            <form class="m-2 border-b border-secondary" on:submit={newNode}>
                <div class="flex gap-2">
                    <input
                        type="text"
                        bind:value={keyInput}
                        placeholder="Key"
                        class={cx(
                            "bg-primary border border-secondary rounded-md text-sm px-1 flex-1",
                            genRandomKey && "opacity-30 pointer-events-none"
                        )}
                    />
                    <div class="flex gap-1">
                        <input
                            type="checkbox"
                            bind:checked={genRandomKey}
                            id="random-key"
                        />
                        <label class="text-sm" for="random-key">Random</label>
                    </div>
                </div>
                <div class="flex justify-between mt-2 mb-3">
                    <div class="flex gap-1">
                        <input
                            type="checkbox"
                            name="make_bootstrap"
                            id="bootstrap"
                            bind:checked={makeBootstrap}
                        />
                        <label
                            class={cx(
                                "text-secondary-text-lighter transition-colors text-sm",
                                makeBootstrap && "text-white"
                            )}
                            for="bootstrap">Make bootstrap node</label
                        >
                    </div>

                    <button
                        type="submit"
                        class="bg-gray-300 hover:bg-white text-black transition-colors rounded-md px-2"
                        >Initialize new node</button
                    >
                </div>
            </form>
        </div>
        <BootstrapNodes />
    </div>
</div>
