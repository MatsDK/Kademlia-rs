<script lang="ts">
    import { type NodeInfo } from "../lib/bindings";
    import { taurpc } from "../lib/ipc";

    export let buckets: NodeInfo["buckets"];
    export let node_id: string;

    const disconnectPeer = async (key: string) => {
        try {
            await taurpc.disconnect_peer(node_id, key);
        } catch (error) {
            console.error(error);
        }
    };
</script>

<div class="flex flex-col overflow-y-scroll h-full w-full absolute text-sm">
    {#each new Array(256).fill(0).map((_, idx) => 255 - idx) as bucket_idx}
        <div class="border-b border-secondary-darker first:pt-2 flex">
            <span class="text-secondary-text-lighter pl-3">{bucket_idx}</span>
            <ul class="w-full">
                {#each buckets[bucket_idx] ?? [] as [key, addr, status]}
                    <div class="flex w-full pl-10">
                        <span class="text-[#999999] min-w-[60%]">
                            {key}
                        </span>
                        <span class="flex-1 text-secondary-text">
                            {addr}
                        </span>
                        <div>
                            <button on:click={() => disconnectPeer(key)}
                                >Disconnect</button
                            >

                            <svg height="20" width="20">
                                <circle
                                    cx="10"
                                    cy="10"
                                    r="6"
                                    stroke="black"
                                    stroke-width="3"
                                    fill={status === "Connected"
                                        ? "green"
                                        : "gray"}
                                />
                            </svg>
                        </div>
                    </div>
                {/each}
            </ul>
        </div>
    {/each}
</div>
