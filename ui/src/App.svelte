<script lang="ts">
    import "./index.css";
    import NodesContainer from "./components/NodesContainer.svelte";
    import { taurpc } from "./lib/ipc";
    import { nodes } from "./lib/store";
    import BootstrapNodes from "./components/BootstrapNodes.svelte";

    const newNode = async () => {
        try {
            const res = await taurpc.new_node();
            nodes.set([...$nodes, res]);
        } catch (e) {
            console.error("Error creating node", e);
        }
    };
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
            <button on:click={newNode}>Initialize new node</button>
        </div>
        <BootstrapNodes />
    </div>
</div>
