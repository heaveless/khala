#!/usr/bin/env python3
"""
RVC Unix Socket Server for Khala real-time translator.

Receives PCM16 24kHz mono audio blocks, runs voice conversion,
returns PCM16 24kHz mono converted audio.

Protocol: length-prefixed binary
  [4 bytes u32 LE: payload_len][payload_len bytes: PCM16 i16 LE]
"""
# Platform workarounds MUST be imported first.
from macos_compat import setup_rvc_paths  # noqa: E402

import argparse
import asyncio
import os

from processor import RvcProcessor, load_rvc_config
from server import serve


def parse_args():
    parser = argparse.ArgumentParser(description="RVC Unix Socket Server for Khala")
    parser.add_argument("--socket", required=True, help="Unix socket path")
    parser.add_argument("--model", required=True)
    parser.add_argument("--index", default="")
    parser.add_argument("--pitch", type=int, default=0)
    parser.add_argument("--index-rate", type=float, default=0.3)
    parser.add_argument(
        "--f0method",
        default="rmvpe",
        choices=["pm", "harvest", "crepe", "rmvpe"],
    )
    parser.add_argument("--block-time", type=float, default=0.1)
    parser.add_argument("--extra-time", type=float, default=2.5)
    parser.add_argument("--crossfade-time", type=float, default=0.05)
    parser.add_argument("--rvc-lib", required=True, help="Path to RVC library directory")
    parser.add_argument("--hubert", required=True, help="Path to hubert_base.pt")
    parser.add_argument("--rmvpe", required=True, help="Path to rmvpe.pt")
    return parser.parse_args()


def main():
    args = parse_args()
    setup_rvc_paths(args.rvc_lib)

    config = load_rvc_config()
    print(f"Device: {config.device}, Half: {config.is_half}")

    processor = RvcProcessor(
        pth_path=args.model,
        index_path=args.index,
        config=config,
        hubert_path=args.hubert,
        rmvpe_path=args.rmvpe,
        pitch=args.pitch,
        index_rate=args.index_rate,
        block_time=args.block_time,
        crossfade_time=args.crossfade_time,
        extra_time=args.extra_time,
        f0method=args.f0method,
    )

    asyncio.run(serve(args.socket, processor))


if __name__ == "__main__":
    main()
