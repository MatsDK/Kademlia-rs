<script lang="ts">
    import { taurpc } from "../lib/ipc";

    export let node_key: string;
    export let record_key: string;
    export let publisher: string;
    export let value: string;

    const is_publisher = publisher === node_key;

    const removeRecord = async () => {
        try {
            await taurpc.remove_record(node_key, record_key);
        } catch (error) {
            console.error(error);
        }
    };
</script>

<div
    class="group border rounded-sm border-secondary px-2 overflow-hidden text-sm"
>
    <div class="max-w-full truncate font-semibold">
        {record_key}
    </div>
    <pre class="truncate max-w-full text-secondary-text-lighter">

                            {value}
                    </pre>
    {#if is_publisher}
        <div class="flex justify-end gap-1 items-center pb-px">
            <button
                class="opacity-0 group-hover:opacity-60 transition-opacity text-xs"
                on:click={removeRecord}>Remove</button
            >
            <span
                class="bg-gray-300 text-black text-xs px-1 rounded-md font-semibold h-[16px]"
                >Publisher</span
            >
        </div>
    {/if}
</div>
