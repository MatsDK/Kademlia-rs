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
                    {@const isConnected = status === "Connected"}
                    <div class="flex w-full pl-10 group">
                        <span class="text-[#999999] min-w-[60%]">
                            {key}
                        </span>
                        <span class="flex-1 text-secondary-text">
                            {addr}
                        </span>
                        <div class="flex items-center">
                            {#if isConnected}
                                <button
                                    title="Disconnect"
                                    on:click={() => disconnectPeer(key)}
                                    ><svg
                                        class="w-3 h-3 fill-white opacity-0 group-hover:opacity-60"
                                        xmlns="http://www.w3.org/2000/svg"
                                        fill="#000000"
                                        viewBox="0 0 1024 1024"
                                    >
                                        <path
                                            d="M832.6 191.4c-84.6-84.6-221.5-84.6-306 0l-96.9 96.9 51 51 96.9-96.9c53.8-53.8 144.6-59.5 204 0 59.5 59.5 53.8 150.2 0 204l-96.9 96.9 51.1 51.1 96.9-96.9c84.4-84.6 84.4-221.5-.1-306.1zM446.5 781.6c-53.8 53.8-144.6 59.5-204 0-59.5-59.5-53.8-150.2 0-204l96.9-96.9-51.1-51.1-96.9 96.9c-84.6 84.6-84.6 221.5 0 306s221.5 84.6 306 0l96.9-96.9-51-51-96.8 97zM260.3 209.4a8.03 8.03 0 0 0-11.3 0L209.4 249a8.03 8.03 0 0 0 0 11.3l554.4 554.4c3.1 3.1 8.2 3.1 11.3 0l39.6-39.6c3.1-3.1 3.1-8.2 0-11.3L260.3 209.4z"
                                        />
                                    </svg></button
                                >
                            {/if}
                            <svg height="20" width="20">
                                <circle
                                    cx="10"
                                    cy="10"
                                    r="6"
                                    stroke="black"
                                    stroke-width="3"
                                    fill={isConnected ? "green" : "gray"}
                                />
                            </svg>
                        </div>
                    </div>
                {/each}
            </ul>
        </div>
    {/each}
</div>
