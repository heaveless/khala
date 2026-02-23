"""
macOS Apple Silicon platform workarounds for RVC.

MUST be imported before any other library in the application entry point.
Sets environment variables and forces specific import order to prevent
segfaults caused by OpenMP/threading conflicts between FAISS and PyTorch MPS.
"""
import os
import sys

os.environ["OMP_NUM_THREADS"] = "1"
os.environ["MKL_NUM_THREADS"] = "1"

# faiss must be imported BEFORE torch on Apple Silicon to avoid
# a segfault from conflicting OpenMP initialization.
import faiss  # noqa: F401, E402
import torch  # noqa: E402

# PyTorch 2.6+ defaults to weights_only=True for torch.load().
# RVC checkpoints contain arbitrary Python objects, so we disable this.
_original_torch_load = torch.load
torch.load = lambda *a, **kw: _original_torch_load(
    *a, **{**kw, "weights_only": False}
)


def setup_rvc_paths(rvc_lib_dir: str) -> None:
    """Add the vendored RVC library to sys.path."""
    rvc_lib_dir = os.path.abspath(rvc_lib_dir)
    if rvc_lib_dir not in sys.path:
        sys.path.insert(0, rvc_lib_dir)

    os.chdir(rvc_lib_dir)
