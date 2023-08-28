<script lang="ts">
    import "./index.css";
    import NodesContainer from "./components/NodesContainer.svelte";
    import { taurpc } from "./lib/ipc";
    import { bootstrap_nodes, nodes } from "./lib/store";
    import { onMount } from "svelte";

    const newNode = async () => {
        try {
            const res = await taurpc.new_node();
            nodes.set([...$nodes, res]);
        } catch (e) {
            console.error("Error creating node", e);
        }
    };

    onMount(() => {
        const unlisten = taurpc.bootstrap_nodes_changed.on(
            (new_bootstrap_nodes) => {
                bootstrap_nodes.set(new_bootstrap_nodes);
            }
        );

        return unlisten;
    });
</script>

<div class="flex flex-col w-screen h-screen bg-primary text-white">
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
            <button on:click={newNode}>Initialize new node</button>
        </div>
        <div class="w-[60%] border-l border-secondary py-1 px-2">
            <div class="flex justify-between items-center">
                <span class="text-lg">Bootstrap nodes</span>
                <button class="text-gray-700 hover:text-white transition-colors"
                    >Add bootstrap node</button
                >
            </div>
            <ul>
                {#each $bootstrap_nodes as [key, addr]}
                    <li class="flex gap-4">
                        <span class="font-semibold">{key}</span>
                        <span class=" text-gray-700">{addr}</span>
                    </li>
                {/each}
            </ul>
        </div>
    </div>
</div>
