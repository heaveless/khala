"""RVC voice conversion processor for real-time streaming audio."""
import os

import numpy as np
import torch
import torch.nn.functional as F
from torchaudio.transforms import Resample

INPUT_SR = 24000
F0_MIN = 50
F0_MAX = 1100
F0_MEL_MIN = 1127 * np.log(1 + F0_MIN / 700)
F0_MEL_MAX = 1127 * np.log(1 + F0_MAX / 700)
PITCH_CACHE_LEN = 1024


def load_rvc_config():
    """Load RVC's Config singleton, hiding our CLI args from its argparse."""
    import sys

    saved = sys.argv[:]
    sys.argv = [sys.argv[0]]
    from configs.config import Config
    cfg = Config()
    sys.argv = saved
    return cfg


class RvcProcessor:
    """Streaming RVC processor using offline Pipeline components.

    Avoids rtrvc.RVC (which uses multiprocessing.Manager at module level,
    causing segfaults with MPS on macOS due to fork+Metal incompatibility).
    """

    def __init__(
        self,
        pth_path: str,
        index_path: str,
        config,
        *,
        hubert_path: str,
        rmvpe_path: str,
        pitch: int = 0,
        index_rate: float = 0.3,
        block_time: float = 0.25,
        crossfade_time: float = 0.05,
        extra_time: float = 2.5,
        f0method: str = "rmvpe",
    ):
        self.config = config
        self.device = config.device
        self.is_half = config.is_half
        self.f0method = f0method
        self.pitch = pitch
        self.index_rate = index_rate

        self.rmvpe_path = rmvpe_path
        self.hubert_model = self._load_hubert(config, hubert_path)
        self.tgt_sr, self.if_f0, self.version, self.net_g = self._load_synthesizer(
            pth_path
        )
        self.index, self.big_npy = self._load_index(index_path)
        self.rmvpe_model = self._load_f0_model()
        self._init_buffers(block_time, crossfade_time, extra_time)

        print("RvcProcessor ready")

    # --- Model loading ---

    @staticmethod
    def _load_hubert(config, hubert_path: str):
        from fairseq import checkpoint_utils

        print(f"Loading HuBERT model: {hubert_path}")
        models, _, _ = checkpoint_utils.load_model_ensemble_and_task(
            [hubert_path], suffix=""
        )
        model = models[0].to(config.device)
        model = model.half() if config.is_half else model.float()
        print("HuBERT loaded")
        return model.eval()

    def _load_synthesizer(self, pth_path: str):
        from infer.lib.infer_pack.models import (
            SynthesizerTrnMs256NSFsid,
            SynthesizerTrnMs256NSFsid_nono,
            SynthesizerTrnMs768NSFsid,
            SynthesizerTrnMs768NSFsid_nono,
        )

        print(f"Loading voice model: {pth_path}")
        cpt = torch.load(pth_path, map_location="cpu")
        tgt_sr = cpt["config"][-1]
        cpt["config"][-3] = cpt["weight"]["emb_g.weight"].shape[0]
        if_f0 = cpt.get("f0", 1)
        version = cpt.get("version", "v1")
        print(f"Model: version={version}, tgt_sr={tgt_sr}, if_f0={if_f0}")

        synth_class = {
            ("v1", 1): SynthesizerTrnMs256NSFsid,
            ("v1", 0): SynthesizerTrnMs256NSFsid_nono,
            ("v2", 1): SynthesizerTrnMs768NSFsid,
            ("v2", 0): SynthesizerTrnMs768NSFsid_nono,
        }
        net_g = synth_class.get(
            (version, if_f0), SynthesizerTrnMs256NSFsid
        )(*cpt["config"], is_half=self.is_half)
        del net_g.enc_q
        net_g.load_state_dict(cpt["weight"], strict=False)
        net_g.eval().to(self.device)
        net_g = net_g.half() if self.is_half else net_g.float()
        del cpt
        print("Synthesizer loaded")

        return tgt_sr, if_f0, version, net_g

    def _load_index(self, index_path: str):
        if not index_path or not os.path.exists(index_path) or self.index_rate <= 0:
            return None, None

        import faiss

        index = faiss.read_index(index_path)
        big_npy = index.reconstruct_n(0, index.ntotal)
        print(f"Index loaded: {index.ntotal} vectors")
        return index, big_npy

    def _load_f0_model(self):
        if self.if_f0 != 1 or self.f0method != "rmvpe":
            return None

        from infer.lib.rmvpe import RMVPE

        print(f"Loading RMVPE model: {self.rmvpe_path}")
        model = RMVPE(self.rmvpe_path, is_half=self.is_half, device=self.device)
        print("RMVPE loaded")
        return model

    # --- Buffer initialization ---

    def _init_buffers(self, block_time: float, crossfade_time: float, extra_time: float):
        zc = self.tgt_sr // 100

        self.zc = zc
        self.block_frame = int(np.round(block_time * self.tgt_sr / zc)) * zc
        self.block_frame_16k = 160 * self.block_frame // zc
        self.crossfade_frame = int(np.round(crossfade_time * self.tgt_sr / zc)) * zc
        self.sola_buffer_frame = min(self.crossfade_frame, 4 * zc)
        self.sola_search_frame = zc
        self.extra_frame = int(np.round(extra_time * self.tgt_sr / zc)) * zc

        # 16kHz sliding window buffer
        total_tgt = (
            self.extra_frame
            + self.crossfade_frame
            + self.sola_search_frame
            + self.block_frame
        )
        total_16k = 160 * total_tgt // zc
        self.input_wav = torch.zeros(
            total_16k, device=self.device, dtype=torch.float32
        )

        # SOLA crossfade
        self.sola_buffer = torch.zeros(
            self.sola_buffer_frame, device=self.device, dtype=torch.float32
        )
        self.fade_in_window = (
            torch.sin(
                0.5
                * np.pi
                * torch.linspace(
                    0.0,
                    1.0,
                    steps=self.sola_buffer_frame,
                    device=self.device,
                    dtype=torch.float32,
                )
            )
            ** 2
        )
        self.fade_out_window = 1 - self.fade_in_window

        # Resamplers
        self.resample_to_16k = Resample(
            INPUT_SR, 16000, dtype=torch.float32
        ).to(self.device)
        self.resample_to_24k = Resample(
            self.tgt_sr, INPUT_SR, dtype=torch.float32
        ).to(self.device)

        # Inference params (must be tensors for net_g.infer assertions)
        self.skip_head = torch.tensor(
            self.extra_frame // zc, device=self.device
        ).long()
        self.return_length = torch.tensor(
            (self.block_frame + self.sola_buffer_frame + self.sola_search_frame) // zc,
            device=self.device,
        ).long()

        # F0 pitch caches
        self.cache_pitch = torch.zeros(
            PITCH_CACHE_LEN, device=self.device, dtype=torch.long
        )
        self.cache_pitchf = torch.zeros(
            PITCH_CACHE_LEN, device=self.device, dtype=torch.float32
        )

        # Speaker ID
        self.sid = torch.tensor([0], device=self.device).long()

        print(f"Block: {self.block_frame} samples @ {self.tgt_sr}Hz ({block_time}s)")
        print(f"Context: {self.extra_frame} samples ({extra_time}s)")
        print(f"Crossfade: {self.crossfade_frame} ({crossfade_time}s)")

    def reset(self):
        """Reset all streaming buffers for a new response.

        Must be called between responses to prevent old audio context
        from bleeding into the new translation.
        """
        self.input_wav.zero_()
        self.sola_buffer.zero_()
        self.cache_pitch.zero_()
        self.cache_pitchf.zero_()

    # --- Processing ---

    def _extract_f0(self, audio_16k_np):
        """Extract F0 using RMVPE and return coarse pitch + float pitch."""
        f0 = self.rmvpe_model.infer_from_audio(audio_16k_np, thred=0.03)
        f0 *= pow(2, self.pitch / 12)

        f0_mel = 1127 * np.log(1 + f0 / 700)
        f0_mel[f0_mel > 0] = (f0_mel[f0_mel > 0] - F0_MEL_MIN) * 254 / (
            F0_MEL_MAX - F0_MEL_MIN
        ) + 1
        f0_mel[f0_mel <= 1] = 1
        f0_mel[f0_mel > 255] = 255
        f0_coarse = np.rint(f0_mel).astype(np.int32)
        return f0_coarse, f0

    def process_block(self, pcm16_24k: np.ndarray) -> np.ndarray:
        """Process one audio block through RVC voice conversion."""
        audio_float = torch.from_numpy(
            pcm16_24k.astype(np.float32) / 32768.0
        ).to(self.device)

        # Resample 24kHz -> 16kHz
        audio_16k = self.resample_to_16k(audio_float)

        # Slide window: shift left, append new samples
        n_new = audio_16k.shape[0]
        self.input_wav = torch.roll(self.input_wav, -n_new)
        self.input_wav[-n_new:] = audio_16k

        input_wav_np = self.input_wav.cpu().numpy()

        with torch.no_grad():
            infer_wav = self._run_inference(input_wav_np)

        # SOLA crossfade
        output = self._apply_sola(infer_wav)

        # Resample to 24kHz output
        output_24k = self.resample_to_24k(output.unsqueeze(0)).squeeze(0)
        return (output_24k.cpu().numpy() * 32768.0).clip(-32768, 32767).astype(
            np.int16
        )

    def _run_inference(self, input_wav_np: np.ndarray):
        """Extract features, run index search, and synthesize."""
        # HuBERT feature extraction
        feats = self.input_wav.clone()
        if self.is_half:
            feats = feats.half()
        feats = feats.view(1, -1)
        padding_mask = torch.BoolTensor(feats.shape).to(self.device).fill_(False)

        inputs = {
            "source": feats,
            "padding_mask": padding_mask,
            "output_layer": 9 if self.version == "v1" else 12,
        }
        logits = self.hubert_model.extract_features(**inputs)
        feats = (
            self.hubert_model.final_proj(logits[0])
            if self.version == "v1"
            else logits[0]
        )
        feats = torch.cat((feats, feats[:, -1:, :]), 1)

        # Index search
        feats = self._apply_index_search(feats)

        # F0 extraction
        pitch, pitchf = None, None
        if self.if_f0 == 1:
            pitch, pitchf = self._prepare_pitch(input_wav_np)

        # Feature interpolation (2x upsample)
        feats = F.interpolate(feats.permute(0, 2, 1), scale_factor=2).permute(
            0, 2, 1
        )

        # Align lengths
        if pitch is not None:
            p_len = min(feats.shape[1], pitch.shape[1])
            feats = feats[:, :p_len, :]
            pitch = pitch[:, :p_len]
            pitchf = pitchf[:, :p_len]
        p_len = torch.tensor([feats.shape[1]], device=self.device).long()

        # Synthesize
        if self.if_f0 == 1:
            return self.net_g.infer(
                feats,
                p_len,
                pitch,
                pitchf,
                self.sid,
                self.skip_head,
                self.return_length,
                self.return_length,
            )[0][0, 0].float()
        else:
            return self.net_g.infer(
                feats,
                p_len,
                self.sid,
                self.skip_head,
                self.return_length,
                self.return_length,
            )[0][0, 0].float()

    def _apply_index_search(self, feats):
        """Apply FAISS index retrieval to blend voice features."""
        if self.index is None or self.big_npy is None or self.index_rate <= 0:
            return feats

        npy = feats[0].cpu().numpy()
        if self.is_half:
            npy = npy.astype("float32")

        score, ix = self.index.search(npy, k=8)
        if not (ix >= 0).all():
            return feats

        weight = np.square(1 / score)
        weight /= weight.sum(axis=1, keepdims=True)
        npy = np.sum(self.big_npy[ix] * np.expand_dims(weight, axis=2), axis=1)
        if self.is_half:
            npy = npy.astype("float16")

        return (
            torch.from_numpy(npy).unsqueeze(0).to(self.device) * self.index_rate
            + (1 - self.index_rate) * feats
        )

    def _prepare_pitch(self, input_wav_np: np.ndarray):
        """Extract F0, update caches, and return pitch tensors for synthesis."""
        f0_coarse, f0_float = self._extract_f0(input_wav_np)

        f0_len = f0_coarse.shape[0]
        self.cache_pitch = torch.roll(self.cache_pitch, -f0_len)
        self.cache_pitchf = torch.roll(self.cache_pitchf, -f0_len)
        self.cache_pitch[-f0_len:] = (
            torch.from_numpy(f0_coarse).long().to(self.device)
        )
        self.cache_pitchf[-f0_len:] = (
            torch.from_numpy(f0_float).float().to(self.device)
        )

        tail = int(self.return_length.item()) + int(self.skip_head.item())
        pitch = self.cache_pitch[-tail:].unsqueeze(0)
        pitchf = self.cache_pitchf[-tail:].unsqueeze(0)
        return pitch, pitchf

    def _apply_sola(self, infer_wav):
        """Apply SOLA crossfade and extract the output block."""
        conv_input = infer_wav[
            None, None, : self.sola_buffer_frame + self.sola_search_frame
        ]
        cor_nom = F.conv1d(conv_input, self.sola_buffer[None, None, :])
        cor_den = torch.sqrt(
            F.conv1d(
                conv_input**2,
                torch.ones(1, 1, self.sola_buffer_frame, device=self.device),
            )
            + 1e-8
        )
        sola_offset = torch.argmax(cor_nom[0, 0] / cor_den[0, 0]).item()

        infer_wav = infer_wav[sola_offset:]
        infer_wav[: self.sola_buffer_frame] *= self.fade_in_window
        infer_wav[: self.sola_buffer_frame] += self.sola_buffer * self.fade_out_window
        self.sola_buffer[:] = infer_wav[
            self.block_frame : self.block_frame + self.sola_buffer_frame
        ]

        return infer_wav[: self.block_frame]
