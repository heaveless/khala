"""Asyncio Unix socket server for RVC voice conversion."""
import asyncio
import os
import signal
import struct
import traceback

import numpy as np


async def handle_client(reader, writer, processor):
    """Handle a single client connection with length-prefixed binary protocol."""
    print("Client connected")
    try:
        while True:
            len_data = await reader.readexactly(4)
            payload_len = struct.unpack("<I", len_data)[0]

            if payload_len == 0:
                # Zero-length payload = reset signal from client.
                # Clear all streaming buffers for a new response.
                processor.reset()
                writer.write(struct.pack("<I", 0))
                await writer.drain()
                continue

            payload = await reader.readexactly(payload_len)
            samples = np.frombuffer(payload, dtype=np.int16).copy()

            converted = processor.process_block(samples)

            out_bytes = converted.tobytes()
            writer.write(struct.pack("<I", len(out_bytes)))
            writer.write(out_bytes)
            await writer.drain()

    except asyncio.IncompleteReadError:
        print("Client disconnected")
    except Exception as e:
        print(f"Error handling client: {e}")
        traceback.print_exc()
    finally:
        writer.close()


async def serve(socket_path: str, processor) -> None:
    """Run the Unix socket server until SIGINT/SIGTERM."""
    if os.path.exists(socket_path):
        os.unlink(socket_path)

    server = await asyncio.start_unix_server(
        lambda r, w: handle_client(r, w, processor), path=socket_path
    )
    print(f"RVC server listening on {socket_path}")

    loop = asyncio.get_event_loop()
    stop = loop.create_future()
    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, stop.set_result, None)

    try:
        await stop
    finally:
        server.close()
        if os.path.exists(socket_path):
            os.unlink(socket_path)
        print("Server stopped")
