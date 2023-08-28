<script lang="ts">
    import { nodes } from "../lib/store";
</script>

<div class="flex-1 grid grid-cols-fluid gap-2 m-2">
    {#each $nodes as node}
        <div class="border border-secondary px-2 h-full">
            <div class="flex justify-between items-center">
                <span class="font-semibold">
                    {node.key}
                    {#if node.is_bootstrap}
                        <span
                            class="text-xs bg-gray-300 text-black rounded-md px-2 font-semibold py-px whitespace-nowrap"
                            >Bootstrap Node</span
                        >
                    {/if}
                </span>
                <span class="text-gray-700 text-sm">
                    {node.addr}
                </span>
            </div>
        </div>
    {/each}
    {#if $nodes.length === 0}
        <div class="text-secondary">No nodes</div>
    {/if}
</div>
