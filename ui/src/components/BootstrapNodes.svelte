<script lang="ts">
    import { onMount } from "svelte";
    import { taurpc } from "../lib/ipc";
    import { bootstrap_nodes, nodes } from "../lib/store";

    let adding_node = false;
    let node_input = "";

    onMount(() => {
        const unlisten = taurpc.bootstrap_nodes_changed.on(
            (new_bootstrap_nodes) => {
                bootstrap_nodes.set(new_bootstrap_nodes);

                const bootstrap_node_keys = new_bootstrap_nodes.map(
                    ([key, _]) => key
                );
                $nodes.map((node) => {
                    node.is_bootstrap = bootstrap_node_keys.includes(node.key);
                });
                nodes.set($nodes);
            }
        );

        return unlisten;
    });

    const addBootstrapNode = async (e: SubmitEvent) => {
        e.preventDefault();

        if (!node_input.trim()) return;

        await taurpc.add_bootstrap_node(node_input.trim());
        adding_node = false;
        node_input = "";
    };

    const removeBootstrapNode = async (key: string) => {
        await taurpc.remove_bootstrap_node(key);
    };
</script>

<div class="w-[60%] border-l border-secondary py-1 px-2">
    {#if !adding_node}
        <div class="flex justify-between items-center">
            <span class="text-lg">Bootstrap nodes</span>
            <button
                class="text-gray-700 hover:text-white transition-colors"
                on:click={() => (adding_node = true)}>Add bootstrap node</button
            >
        </div>
        <ul class="">
            {#each $bootstrap_nodes as [key, addr]}
                <li class="flex justify-between group">
                    <div class="flex gap-4 w-2/3 justify-between">
                        <span class="font-semibold">{key}</span>
                        <span class=" text-gray-700">{addr}</span>
                    </div>
                    <button
                        class="opacity-0 group-hover:opacity-100 text-gray-700 hover:text-white transition"
                        on:click={() => removeBootstrapNode(key)}
                        >Remove bootstrap node</button
                    >
                </li>
            {/each}
        </ul>
    {:else}
        <form
            class="w-[60%] max-w-[450px] py-20 m-auto flex flex-col"
            on:submit={addBootstrapNode}
        >
            <label for="key">Node key</label>
            <input
                type="text"
                id="key"
                bind:value={node_input}
                class="bg-primary border border-secondary rounded-md text-sm px-1"
            />
            <div class="flex justify-between mt-2">
                <button
                    class="text-gray-700 hover:text-white transition-colors"
                    on:click={() => (adding_node = false)}>Cancel</button
                >
                <button
                    type="submit"
                    class="bg-gray-300 hover:bg-white text-black transition-colors rounded-md px-2"
                    >Add node</button
                >
            </div>
        </form>
    {/if}
</div>
