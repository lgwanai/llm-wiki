#!/usr/bin/env python3
import base64
import json
import os
import re
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image


def normalize_lang(language):
    lang = (language or "ch").lower()
    if "chi" in lang or "ch" in lang or "zh" in lang:
        return "ch"
    if "eng" in lang or "en" in lang:
        return "en"
    return "ch"


def normalize_device(device):
    device = (device or "auto").lower()
    if device == "auto":
        return None
    return device


def build_paddleocr(req):
    from paddleocr import PaddleOCR

    kwargs = {
        "lang": normalize_lang(req.get("language")),
    }
    device = normalize_device(req.get("device"))
    if device:
        kwargs["device"] = device

    model = req.get("model") or ""
    if model and model.lower() not in {"default", "pp-ocrv5_server"}:
        # PaddleOCR 3.x accepts model name overrides for PP-OCR pipelines. Older
        # versions ignore unknown kwargs via the fallback below.
        kwargs["text_recognition_model_name"] = model

    try:
        return PaddleOCR(**kwargs)
    except TypeError:
        kwargs.pop("device", None)
        kwargs.pop("text_recognition_model_name", None)
        return PaddleOCR(**kwargs)


def build_prompt(processor, query):
    messages = [
        {"role": "user", "content": [
            {"type": "image"},
            {"type": "text", "text": query}
        ]}
    ]
    return processor.apply_chat_template(messages, tokenize=False, add_generation_prompt=True)


def line_box_items(text, width, height):
    lines = [line.strip() for line in (text or "").splitlines() if line.strip()]
    if not lines:
        return []
    margin_x = max(1.0, width * 0.04)
    top = max(1.0, height * 0.06)
    usable_h = max(1.0, height - top * 2)
    line_h = usable_h / max(len(lines), 1)
    items = []
    for idx, line in enumerate(lines):
        y1 = top + idx * line_h
        y2 = min(height - 1.0, y1 + line_h * 0.8)
        bbox = [float(margin_x), float(y1), float(width - margin_x), float(y2)]
        items.append({
            "text": line,
            "bbox": bbox,
            "confidence": 0.75,
            "polygon": [[bbox[0], bbox[1]], [bbox[2], bbox[1]], [bbox[2], bbox[3]], [bbox[0], bbox[3]]],
        })
    return items


def parse_json_boxes(text, width, height):
    raw = (text or "").strip()
    if raw.startswith("```"):
        raw = raw.strip("`")
        if raw.lower().startswith("json"):
            raw = raw[4:].strip()
    try:
        data = json.loads(raw)
    except Exception:
        return None
    if isinstance(data, dict):
        data = data.get("results") or data.get("items") or data.get("lines")
    if not isinstance(data, list):
        return None
    items = []
    for row in data:
        if not isinstance(row, dict):
            continue
        text_val = row.get("text") or row.get("content") or ""
        bbox = row.get("bbox") or row.get("box")
        if not text_val or not isinstance(bbox, list) or len(bbox) < 4:
            continue
        box = [float(v) for v in bbox[:4]]
        if max(box) <= 1.5:
            box = [box[0] * width, box[1] * height, box[2] * width, box[3] * height]
        items.append({
            "text": str(text_val),
            "bbox": box,
            "confidence": float(row.get("confidence") or row.get("score") or 0.85),
            "polygon": row.get("polygon"),
        })
    return items


def box_polygon(box):
    return [[box[0], box[1]], [box[2], box[1]], [box[2], box[3]], [box[0], box[3]]]


def normalize_box(box, width, height):
    vals = [float(v) for v in box[:4]]
    if max(vals) <= 1.5:
        vals = [vals[0] * width, vals[1] * height, vals[2] * width, vals[3] * height]
    x1, y1, x2, y2 = vals
    return [min(x1, x2), min(y1, y2), max(x1, x2), max(y1, y2)]


def loc_to_pixel(value, extent):
    return float(value) / 999.0 * float(extent)


def parse_spotting_output(text, width, height):
    items = []
    loc_re = re.compile(r"<\|LOC_(\d{1,3})\|>")
    marker_re = re.compile(r"<\|LOC_(?:BEGIN|SEP|END)\|>")

    for raw_line in (text or "").splitlines():
        nums = [int(n) for n in loc_re.findall(raw_line)]
        if len(nums) < 8:
            continue
        label = loc_re.sub("", raw_line)
        label = marker_re.sub("", label).strip()
        if not label:
            continue
        coords = nums[:8]
        points = []
        for idx in range(0, 8, 2):
            x = loc_to_pixel(coords[idx], width)
            y = loc_to_pixel(coords[idx + 1], height)
            points.append([x, y])
        xs = [p[0] for p in points]
        ys = [p[1] for p in points]
        items.append({
            "text": label,
            "bbox": [min(xs), min(ys), max(xs), max(ys)],
            "confidence": 0.9,
            "polygon": points,
        })
    return items


def run_paddleocr_vl(req, image_path):
    from mlx_vlm import load, generate

    model_path = req.get("model") or req.get("model_dir")
    model, processor = load(model_path)
    if hasattr(processor, "image_processor"):
        image_processor = processor.image_processor
        image_processor.max_pixels = max(
            int(getattr(image_processor, "max_pixels", 0) or 0),
            1605632,
        )
        min_pixels = int(getattr(image_processor, "min_pixels", 112896) or 112896)
        image_processor.size = {
            "shortest_edge": min_pixels,
            "longest_edge": image_processor.max_pixels,
        }

    prompt = build_prompt(processor, "Spotting:")
    result = generate(
        model,
        processor,
        prompt=prompt,
        image=image_path,
        max_tokens=1024,
        temperature=0.0,
    )
    text = result.text.strip()
    items = parse_spotting_output(text, int(req["width"]), int(req["height"]))
    if not items:
        items = parse_json_boxes(text, int(req["width"]), int(req["height"]))
    if items is None:
        items = line_box_items(text, int(req["width"]), int(req["height"]))
    return items


def walk_mineru_nodes(node):
    if isinstance(node, dict):
        yield node
        for value in node.values():
            yield from walk_mineru_nodes(value)
    elif isinstance(node, list):
        for value in node:
            yield from walk_mineru_nodes(value)


def parse_mineru_middle(data, width, height):
    items = []
    for node in walk_mineru_nodes(data):
        content = node.get("content") or node.get("text")
        bbox = node.get("bbox")
        if not content or not isinstance(bbox, list) or len(bbox) < 4:
            continue
        box = normalize_box(bbox, width, height)
        items.append({
            "text": str(content),
            "bbox": box,
            "confidence": float(node.get("score") or node.get("confidence") or 0.9),
            "polygon": box_polygon(box),
        })
    return items


def parse_coordinate_text(text, width, height):
    parsed = parse_spotting_output(text, width, height)
    if parsed:
        return parsed
    parsed = parse_json_boxes(text, width, height)
    if parsed:
        return parsed

    items = []
    box_re = re.compile(
        r"(?P<label>[^\n<>\[\]\{\}:：]{1,120})[:：]?\s*"
        r"(?:bbox|box|det|quad|polygon)?\s*[:=]?\s*"
        r"[\[\(]\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*,\s*"
        r"(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)(?:\s*,\s*"
        r"(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*,\s*"
        r"(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?))?\s*[\]\)]"
    )
    for match in box_re.finditer(text or ""):
        label = match.group("label").strip(" -|,，")
        label = re.sub(r"\b(?:bbox|box|det|quad|polygon)\b\s*$", "", label, flags=re.IGNORECASE).strip(" -|,，")
        nums = [float(v) for v in match.groups()[1:] if v is not None]
        if not label or len(nums) < 4:
            continue
        if len(nums) >= 8:
            points = []
            for idx in range(0, 8, 2):
                x = nums[idx]
                y = nums[idx + 1]
                if max(nums[:8]) <= 1.5:
                    x *= width
                    y *= height
                points.append([float(x), float(y)])
            xs = [p[0] for p in points]
            ys = [p[1] for p in points]
            box = [min(xs), min(ys), max(xs), max(ys)]
            poly = points[:4]
        else:
            box = normalize_box(nums, width, height)
            poly = box_polygon(box)
        items.append({"text": label, "bbox": box, "confidence": 0.82, "polygon": poly})
    return items


def run_mineru(req, image_path):
    width = int(req["width"])
    height = int(req["height"])
    model_dir = req.get("model_dir") or ""
    with tempfile.TemporaryDirectory() as out_dir:
        env = os.environ.copy()
        env.setdefault("MINERU_MODEL_SOURCE", "modelscope")
        if model_dir:
            env.setdefault("MINERU_MODEL_ROOT", model_dir)
            env.setdefault("MINERU_HOME", model_dir)
        cmd = [sys.executable, "-m", "mineru.cli.client", "-p", image_path, "-o", out_dir, "-b", "pipeline"]
        proc = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
        if proc.returncode != 0:
            raise RuntimeError(
                "MinerU runtime failed. Install MinerU in the Python environment selected by "
                "LLM_WIKI_PYTHON or place a complete MinerU runtime on PATH.\n" + proc.stderr
            )
        items = []
        for root, _, files in os.walk(out_dir):
            for name in files:
                if not name.endswith("_middle.json"):
                    continue
                path = os.path.join(root, name)
                with open(path, "r", encoding="utf-8") as f:
                    items.extend(parse_mineru_middle(json.load(f), width, height))
        return items


def parse_deepseek_files(out_dir, width, height):
    items = []
    for root, _, files in os.walk(out_dir):
        for name in files:
            path = os.path.join(root, name)
            lower = name.lower()
            if lower.endswith(".json"):
                with open(path, "r", encoding="utf-8") as f:
                    parsed = parse_json_boxes(f.read(), width, height)
                if parsed:
                    items.extend(parsed)
            elif lower.endswith((".txt", ".md")):
                with open(path, "r", encoding="utf-8") as f:
                    parsed = parse_coordinate_text(f.read(), width, height)
                if parsed:
                    items.extend(parsed)
    return items


def run_deepseek_ocr(req, image_path):
    width = int(req["width"])
    height = int(req["height"])
    model_path = req.get("model") or req.get("model_dir")
    if not model_path or not os.path.exists(model_path):
        raise RuntimeError("DeepSeek-OCR model directory does not exist: " + str(model_path))

    try:
        import torch
        from transformers import AutoModel, AutoTokenizer
    except Exception as exc:
        raise RuntimeError(
            "DeepSeek-OCR requires torch and transformers in the Python environment selected by LLM_WIKI_PYTHON."
        ) from exc

    device = normalize_device(req.get("device")) or ("cuda" if torch.cuda.is_available() else "cpu")
    tokenizer = AutoTokenizer.from_pretrained(model_path, trust_remote_code=True)
    model = AutoModel.from_pretrained(model_path, trust_remote_code=True)
    model = model.eval()
    if hasattr(model, "to"):
        model = model.to(device)

    prompt = "<image>\n<|grounding|>Convert the document to markdown."
    with tempfile.TemporaryDirectory() as out_dir:
        if hasattr(model, "infer"):
            model.infer(
                tokenizer,
                prompt=prompt,
                image_file=image_path,
                output_path=out_dir,
                base_size=1024,
                image_size=640,
                crop_mode=True,
                save_results=True,
            )
            items = parse_deepseek_files(out_dir, width, height)
            if items:
                return items
        raise RuntimeError(
            "DeepSeek-OCR model loaded, but no supported grounding output was produced. "
            "Use a DeepSeek-OCR model that exposes infer(..., save_results=True)."
        )


def as_box(poly):
    pts = np.asarray(poly, dtype=float).reshape(-1, 2)
    x_min = float(np.min(pts[:, 0]))
    y_min = float(np.min(pts[:, 1]))
    x_max = float(np.max(pts[:, 0]))
    y_max = float(np.max(pts[:, 1]))
    return [x_min, y_min, x_max, y_max]


def as_polygon(poly):
    pts = np.asarray(poly, dtype=float).reshape(-1, 2)
    if pts.shape[0] < 4:
        return None
    return [[float(x), float(y)] for x, y in pts[:4]]


def parse_predict_result(result):
    items = []
    if not isinstance(result, list):
        result = [result]

    for page in result:
        data = page
        if hasattr(page, "json"):
            data = page.json
        if isinstance(data, dict) and "res" in data:
            data = data["res"]

        if isinstance(data, dict) and "rec_texts" in data:
            texts = data.get("rec_texts") or []
            scores = data.get("rec_scores") or []
            boxes = data.get("rec_boxes") or []
            polys = data.get("rec_polys") or []
            for idx, text in enumerate(texts):
                poly = polys[idx] if idx < len(polys) else None
                box = boxes[idx] if idx < len(boxes) else (as_box(poly) if poly is not None else [0, 0, 1, 1])
                items.append(
                    {
                        "text": str(text),
                        "bbox": [float(v) for v in np.asarray(box, dtype=float).reshape(-1)[:4]],
                        "confidence": float(scores[idx]) if idx < len(scores) else 1.0,
                        "polygon": as_polygon(poly) if poly is not None else None,
                    }
                )
            continue

        # PaddleOCR 2.x shape: [[box, (text, score)], ...]
        if isinstance(data, list):
            for row in data:
                if not row:
                    continue
                if isinstance(row, list) and len(row) == 1 and isinstance(row[0], list):
                    row = row[0]
                if isinstance(row, list) and len(row) >= 2:
                    poly = row[0]
                    rec = row[1]
                    text = rec[0] if isinstance(rec, (list, tuple)) and rec else ""
                    score = rec[1] if isinstance(rec, (list, tuple)) and len(rec) > 1 else 1.0
                    items.append(
                        {
                            "text": str(text),
                            "bbox": as_box(poly),
                            "confidence": float(score),
                            "polygon": as_polygon(poly),
                        }
                    )
    return items


def main():
    req = json.load(sys.stdin)
    os.environ.setdefault("PADDLEOCR_HOME", req.get("model_dir") or "")

    width = int(req["width"])
    height = int(req["height"])
    raw = base64.b64decode(req["rgb_base64"])
    image = Image.frombytes("RGB", (width, height), raw)

    with tempfile.NamedTemporaryFile(suffix=".png", delete=False) as f:
        temp_path = f.name
        image.save(temp_path)

    try:
        if req.get("engine") == "paddleocr-vl":
            items = run_paddleocr_vl(req, temp_path)
        elif req.get("engine") == "mineru":
            items = run_mineru(req, temp_path)
        elif req.get("engine") == "deepseek-ocr":
            items = run_deepseek_ocr(req, temp_path)
        else:
            ocr = build_paddleocr(req)
            if hasattr(ocr, "predict"):
                result = ocr.predict(input=temp_path)
            else:
                result = ocr.ocr(temp_path, cls=True)
            items = parse_predict_result(result)
        print(json.dumps({"results": items}, ensure_ascii=False))
    finally:
        try:
            os.unlink(temp_path)
        except OSError:
            pass


if __name__ == "__main__":
    main()
