from dataclasses import dataclass
from pathlib import Path

import numpy as np
import pytest
from PIL import Image

from local_image_filter.inference.base import InferenceError
from local_image_filter.inference.mobileclip import MobileCLIPClassifier
from local_image_filter.inference.nudenet_decoder import NudeNetYoloV8Decoder
from local_image_filter.inference.onnx_session import OnnxSession
from local_image_filter.inference.yolox_decoder import YoloXDecoder


@dataclass
class Node:
    name: str
    shape: list[int]
    type: str = "tensor(float)"


class Session:
    def __init__(self, output: np.ndarray, input_name: str = "images") -> None:
        self.output = output
        self.inputs = [Node(input_name, [1, 3, 416, 416])]
        self.outputs = [Node("output", list(output.shape))]
        self.last_feeds = None

    def run(self, feeds, output_names=None):
        self.last_feeds = feeds
        return [self.output]


def test_yolox_objectness_person_and_reverse_mapping() -> None:
    output = np.zeros((1, 1, 85), dtype=np.float32)
    output[0, 0, :5] = [208, 208, 104, 208, 0.9]
    output[0, 0, 5] = 0.8
    session = Session(output)
    decoder = YoloXDecoder(
        session, input_size=416, class_count=80, confidence_threshold=0.3, iou_threshold=0.5
    )
    results = decoder.detect(Image.new("RGB", (416, 416), (255, 0, 0)))
    assert len(results) == 1
    assert results[0].class_index == 0
    assert results[0].confidence == pytest.approx(0.72)
    assert results[0].x == pytest.approx(0.375)
    assert results[0].height == pytest.approx(0.5)
    assert session.last_feeds["images"][0, :, 0, 0].tolist() == [0.0, 0.0, 255.0]


def test_yolox_invalid_shape_and_nonfinite_output() -> None:
    session = Session(np.zeros((1, 85, 10), dtype=np.float32))
    decoder = YoloXDecoder(
        session, input_size=416, class_count=80, confidence_threshold=0.3, iou_threshold=0.5
    )
    with pytest.raises(InferenceError, match="expected"):
        decoder.detect(Image.new("RGB", (100, 100)))
    output = np.zeros((1, 1, 85), dtype=np.float32)
    output[0, 0, 0] = np.nan
    session.output = output
    with pytest.raises(InferenceError, match="NaN"):
        decoder.detect(Image.new("RGB", (100, 100)))


def test_nudenet_yolov8_layout_confidence_and_class_index() -> None:
    output = np.zeros((1, 22, 2), dtype=np.float32)
    output[0, :4, 0] = [160, 160, 100, 80]
    output[0, 4 + 3, 0] = 0.75
    session = Session(output, input_name="images")
    session.inputs[0].shape = [1, 3, 320, 320]
    decoder = NudeNetYoloV8Decoder(
        session, input_size=320, class_count=18, confidence_threshold=0.2, iou_threshold=0.45
    )
    results = decoder.detect(Image.new("RGB", (640, 320), "white"))
    assert len(results) == 1
    assert results[0].class_index == 3
    assert results[0].confidence == pytest.approx(0.75)
    assert 0 <= results[0].x <= 1
    assert 0 <= results[0].y <= 1


def test_nudenet_rejects_ambiguous_output_layout() -> None:
    session = Session(np.zeros((1, 2100, 22), dtype=np.float32))
    decoder = NudeNetYoloV8Decoder(
        session, input_size=320, class_count=18, confidence_threshold=0.2, iou_threshold=0.45
    )
    with pytest.raises(InferenceError, match="expected"):
        decoder.detect(Image.new("RGB", (320, 320)))


def test_mobileclip_preprocessing_softmax_and_class_order(tmp_path: Path) -> None:
    embeddings = np.eye(4, 512, dtype=np.float32)
    prompt_path = tmp_path / "prompts.npz"
    np.savez(
        prompt_path,
        class_names=np.asarray(["woman", "man", "uncertain", "notPerson"]),
        embeddings=embeddings,
    )
    output = np.zeros((1, 512), dtype=np.float32)
    output[0, 0] = 1
    session = Session(output, input_name="image")
    session.inputs[0].shape = [1, 3, 256, 256]
    session.outputs[0] = Node("embedding", [1, 512])
    classifier = MobileCLIPClassifier(
        session,
        {
            "adapter": "mobileclip2-image",
            "input_size": 256,
            "input_name": "image",
            "output_name": "embedding",
            "class_names": ["woman", "man", "uncertain", "notPerson"],
            "image_scale": 1 / 255,
            "mean": [0, 0, 0],
            "std": [1, 1, 1],
            "temperature": 100,
        },
        prompt_path,
    )
    result = classifier.classify(Image.new("RGB", (400, 200), (255, 0, 0)), "p1", "c1")
    assert result.predicted_class == "woman"
    assert sum(result.scores.values()) == pytest.approx(1)
    tensor = session.last_feeds["image"]
    assert tensor.shape == (1, 3, 256, 256)
    assert tensor[0, :, 0, 0].tolist() == [1.0, 0.0, 0.0]


def test_mobileclip_rejects_dimension_mismatch(tmp_path: Path) -> None:
    path = tmp_path / "prompts.npz"
    np.savez(path, class_names=np.asarray(["woman"]), embeddings=np.ones((1, 5)))
    session = Session(np.ones((1, 5)), input_name="image")
    session.outputs[0] = Node("embedding", [1, 5])
    with pytest.raises(InferenceError, match="dimension 512"):
        MobileCLIPClassifier(
            session,
            {"adapter": "mobileclip2-image", "class_names": ["woman"]},
            path,
        )


def test_onnx_warm_up_accepts_concrete_shape_for_dynamic_input() -> None:
    class DynamicInput:
        name = "images"
        shape = ["batch", 3, "height", "width"]
        type = "tensor(float)"

    class FakeSession:
        inputs = [DynamicInput()]

        def run(self, feeds, output_names=None):
            tensor = feeds["images"]
            assert tensor.shape == (1, 3, 320, 320)
            return [np.zeros((1, 22, 2100), dtype=np.float32)]

    OnnxSession.warm_up(FakeSession(), {"images": (1, 3, 320, 320)})
