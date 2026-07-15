#!/usr/bin/env python3
import json
import pathlib
import sys

checkpoint = pathlib.Path(sys.argv[1]).resolve()
sha256 = sys.argv[2]
manifest = {
    "model": "MobileCLIP2-S2",
    "source": "https://huggingface.co/apple/MobileCLIP2-S2",
    "checkpointFile": "mobileclip2_s2.pt",
    "checkpointSHA256": sha256,
    "checkpointPath": str(checkpoint),
    "license": "Apple Machine Learning Research Model License Agreement",
}
pathlib.Path("model_manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
