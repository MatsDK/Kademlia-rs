<script lang="ts">
    import { nodes } from "../lib/store";
    import Node from "./Node.svelte";
</script>

<div class="flex-1 grid grid-cols-fluid gap-2 m-2">
    {#each $nodes as node}
        <Node {node} />
    {/each}
    {#if $nodes.length === 0}
        <div class="text-secondary">No nodes</div>
    {/if}
</div>
