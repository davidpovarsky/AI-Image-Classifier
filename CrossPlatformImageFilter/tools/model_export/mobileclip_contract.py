from __future__ import annotations

import ast
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass(frozen=True, slots=True)
class MobileCLIPContract:
    model_id: str
    model_name: str
    image_size: int
    prompts: dict[str, list[str]]

    @property
    def class_names(self) -> list[str]:
        return list(self.prompts)


def load_ios_contract(path: Path) -> MobileCLIPContract:
    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    values: dict[str, Any] = {}
    wanted = {"MODEL_ID", "MODEL_NAME", "IMAGE_SIZE", "PROMPTS"}
    for statement in tree.body:
        if not isinstance(statement, (ast.Assign, ast.AnnAssign)):
            continue
        targets = statement.targets if isinstance(statement, ast.Assign) else [statement.target]
        value_node = statement.value
        for target in targets:
            if isinstance(target, ast.Name) and target.id in wanted:
                values[target.id] = ast.literal_eval(value_node)
    missing = wanted - values.keys()
    if missing:
        raise ValueError(f"Missing MobileCLIP constants in {path}: {sorted(missing)}")
    prompts = values["PROMPTS"]
    if not isinstance(prompts, dict) or list(prompts) != ["woman", "man", "uncertain", "notPerson"]:
        raise ValueError("Unexpected MobileCLIP prompt class order")
    if not all(
        isinstance(items, list) and all(isinstance(item, str) for item in items)
        for items in prompts.values()
    ):
        raise ValueError("MobileCLIP prompts must be lists of strings")
    return MobileCLIPContract(
        model_id=str(values["MODEL_ID"]),
        model_name=str(values["MODEL_NAME"]),
        image_size=int(values["IMAGE_SIZE"]),
        prompts=prompts,
    )
