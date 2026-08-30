#!/usr/bin/env python3
"""One-shot, offline IndexTTS-2.5 acceptance sample using upstream example audio."""

import argparse
import os
import random
import sys
from pathlib import Path

SMOKE_TEXT = "本地语音测试完成。"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--install", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    install = args.install.resolve(strict=True)
    checkpoints = install / "checkpoints"
    reference = install / "examples" / "voice_01.wav"
    if not checkpoints.is_dir() or not reference.is_file():
        raise SystemExit("IndexTTS-2.5 model directory or upstream example audio is missing")

    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    os.environ["TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD"] = "1"
    os.chdir(install)
    sys.path.insert(0, str(install))

    import numpy as np
    import torch
    from indextts.infer_v2_5 import IndexTTS2

    random.seed(42)
    np.random.seed(42)
    torch.manual_seed(42)
    model = IndexTTS2(
        cfg_path=str(checkpoints / "config.yaml"),
        model_dir=str(checkpoints),
        device="mps",
        use_bf16=False,
        use_qwen_emo=False,
    )
    result = model.infer(
        spk_audio_prompt=str(reference),
        text=SMOKE_TEXT,
        output_path=str(args.output),
        lang="ZH",
        use_emo_text=False,
        use_random=False,
        duration_factor=1.0,
        do_sample=True,
        top_p=0.8,
        top_k=30,
        temperature=1.0,
        repetition_penalty=10.0,
        max_mel_tokens=400,
    )
    if result is None or not args.output.is_file():
        raise SystemExit("IndexTTS-2.5 returned no output")
    print(args.output)


if __name__ == "__main__":
    main()
