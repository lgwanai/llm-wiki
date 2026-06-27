#!/usr/bin/env python3
import base64
import json
import math
import os
import re
import shutil
import subprocess
import sys
import tempfile
import importlib.util
import contextlib
import time

import numpy as np
from PIL import Image, ImageOps


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


def worker_options(req):
    options = req.get("options") or {}
    return options if isinstance(options, dict) else {}


def option_value(req, name, default=None):
    options = worker_options(req)
    return options.get(name, req.get(name, default))


def option_bool(req, name, default=False):
    value = option_value(req, name, default)
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        return value.strip().lower() in {"1", "true", "yes", "on"}
    return bool(value)


def option_int(req, name, default):
    try:
        return int(option_value(req, name, default))
    except Exception:
        return int(default)


def option_float(req, name, default):
    try:
        return float(option_value(req, name, default))
    except Exception:
        return float(default)


def unlimited_prompt(req):
    prompt = option_value(req, "prompt")
    if prompt:
        return str(prompt)
    task = str(option_value(req, "task", "document")).strip().lower()
    prompts = {
        "document": "document parsing.",
        "parse": "document parsing.",
        "markdown": "document parsing.",
        "ocr": "Extract the text in the image.",
        "text": "Extract the text in the image.",
        "free": "Free OCR.",
        "figure": "Parse the figure.",
        "table": "document parsing.",
        "formula": "document parsing.",
        "structure": "document parsing.",
    }
    return prompts.get(task, "document parsing.")


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


def clean_model_text(text):
    lines = []
    for line in str(text).splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        if stripped in {"User: <image_placeholder>", "document parsing."}:
            continue
        if stripped.startswith("Assistant:"):
            stripped = stripped[len("Assistant:"):].strip()
            if not stripped:
                continue
        lines.append(stripped)
    return "\n".join(lines).strip()


def dynamic_preprocess_unlimited(image, min_num=2, max_num=32, image_size=640):
    orig_width, orig_height = image.size
    aspect_ratio = orig_width / orig_height
    target_ratios = set()
    for n in range(min_num, max_num + 1):
        for i in range(1, n + 1):
            for j in range(1, n + 1):
                if min_num <= i * j <= max_num:
                    target_ratios.add((i, j))
    target_ratios = sorted(target_ratios, key=lambda x: x[0] * x[1])
    best_ratio_diff = float("inf")
    best_ratio = (1, 1)
    area = orig_width * orig_height
    for ratio in target_ratios:
        target_aspect = ratio[0] / ratio[1]
        ratio_diff = abs(aspect_ratio - target_aspect)
        if ratio_diff < best_ratio_diff:
            best_ratio_diff = ratio_diff
            best_ratio = ratio
        elif ratio_diff == best_ratio_diff:
            if area > 0.5 * image_size * image_size * ratio[0] * ratio[1]:
                best_ratio = ratio

    target_width = image_size * best_ratio[0]
    target_height = image_size * best_ratio[1]
    resized = image.resize((target_width, target_height))
    crops = []
    for i in range(best_ratio[0] * best_ratio[1]):
        col = i % best_ratio[0]
        row = i // best_ratio[0]
        crops.append(resized.crop((
            col * image_size,
            row * image_size,
            (col + 1) * image_size,
            (row + 1) * image_size,
        )))
    return crops, best_ratio


def parse_unlimited_det_output(text, width, height):
    cleaned = re.sub(r"<｜end▁of▁sentence｜>|<\|end▁of▁sentence\|>", "", text or "")
    ref_pattern = re.compile(
        r"<\|ref\|>\s*(?P<label>.*?)\s*<\|/ref\|>\s*"
        r"<\|det\|>\s*(?P<coords>\[[^\]]+\])\s*<\|/det\|>",
        re.DOTALL,
    )
    items = []
    for match in ref_pattern.finditer(cleaned):
        label = match.group("label").strip()
        if not label:
            continue
        try:
            coords_data = json.loads(match.group("coords"))
        except Exception:
            try:
                coords_data = eval(match.group("coords"), {"__builtins__": {}}, {})
            except Exception:
                continue
        if coords_data and isinstance(coords_data[0], (int, float)):
            coords_data = [coords_data]
        for coords in coords_data if isinstance(coords_data, list) else []:
            if not isinstance(coords, (list, tuple)) or len(coords) < 4:
                continue
            vals = [float(v) for v in coords[:4]]
            if max(abs(v) for v in vals) > 1.5:
                vals = [vals[0] / 999.0, vals[1] / 999.0, vals[2] / 999.0, vals[3] / 999.0]
            box = normalize_box(vals, width, height)
            items.append({
                "text": label,
                "bbox": box,
                "confidence": 0.9,
                "polygon": box_polygon(box),
            })

    pattern = re.compile(
        r"<\|det\|>\s*(?P<label>[^\[]*?)\s*"
        r"\[\s*(?P<x1>-?\d+(?:\.\d+)?)\s*,\s*(?P<y1>-?\d+(?:\.\d+)?)\s*,\s*"
        r"(?P<x2>-?\d+(?:\.\d+)?)\s*,\s*(?P<y2>-?\d+(?:\.\d+)?)\s*\]"
        r"<\|/det\|>\s*(?P<content>.*?)(?=<\|det\|>|$)",
        re.DOTALL,
    )
    for match in pattern.finditer(cleaned):
        content = match.group("content").strip()
        if not content:
            content = match.group("label").strip()
        if not content:
            continue
        coords = [float(match.group(name)) for name in ("x1", "y1", "x2", "y2")]
        if max(abs(v) for v in coords) > 1.5:
            coords = [coords[0] / 1000.0, coords[1] / 1000.0, coords[2] / 1000.0, coords[3] / 1000.0]
        box = normalize_box(coords, width, height)
        items.append({
            "text": content,
            "bbox": box,
            "confidence": 0.9,
            "polygon": box_polygon(box),
        })
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


def run_unlimited_ocr_torch(req, image_path, model_path):
    code_path = os.path.join(os.path.dirname(model_path), "Unlimited-OCR-code")
    required = ["modeling_unlimitedocr.py", "deepencoder.py", "modeling_deepseekv2.py", "config.json"]
    if not os.path.isdir(code_path) or any(not os.path.exists(os.path.join(code_path, name)) for name in required):
        raise RuntimeError(
            "PaddlePaddle/Unlimited-OCR code files are missing. Enable ocr.auto_download "
            "or download them under the OCR model root as Unlimited-OCR-code."
        )

    try:
        import torch
        import torch.nn as torch_nn
        from safetensors import safe_open
        from transformers import AutoTokenizer
        from transformers.generation.utils import GenerationMixin
    except Exception as exc:
        raise RuntimeError(
            "Unlimited-OCR compatibility runtime requires torch, safetensors and transformers."
        ) from exc

    requested_device = normalize_device(req.get("device"))
    if requested_device in {None, "auto", "mps"} and torch.backends.mps.is_available():
        device = "mps"
    elif requested_device == "cpu":
        device = "cpu"
    else:
        raise RuntimeError("Unlimited-OCR compatibility runtime requires Apple MPS or device=cpu.")

    package_root = tempfile.mkdtemp(prefix="unlimited_ocr_code_")
    package_dir = os.path.join(package_root, "unlimited_ocr_code")
    os.makedirs(package_dir, exist_ok=True)
    for name in os.listdir(code_path):
        if name.endswith((".py", ".json")):
            shutil.copy2(os.path.join(code_path, name), os.path.join(package_dir, name))
    with open(os.path.join(package_dir, "__init__.py"), "w", encoding="utf-8") as f:
        f.write("")

    old_path = list(sys.path)
    old_cuda = getattr(torch.Tensor, "cuda", None)
    try:
        sys.path.insert(0, package_root)
        from unlimited_ocr_code.modeling_unlimitedocr import (
            BasicImageTransform,
            SlidingWindowNoRepeatNgramProcessor,
            UnlimitedOCRConfig,
            UnlimitedOCRForCausalLM,
            dynamic_preprocess,
            text_encode,
        )

        # The original code was published for CUDA/Transformers<4.50. Patch the
        # two compatibility points needed for local Apple Silicon inference.
        torch.Tensor.cuda = lambda self, *args, **kwargs: self
        torch_nn.Linear.reset_parameters = lambda self: None
        torch_nn.LayerNorm.reset_parameters = lambda self: None
        patched_cls = type(
            "PatchedUnlimitedOCRForCausalLM",
            (UnlimitedOCRForCausalLM, GenerationMixin),
            {},
        )

        model = patched_cls(UnlimitedOCRConfig.from_pretrained(code_path)).eval()
        weights_path = os.path.join(model_path, "model.safetensors")
        if not os.path.exists(weights_path):
            raise RuntimeError("Unlimited-OCR-MLX weights are missing: " + weights_path)

        state = {}
        with safe_open(weights_path, framework="pt", device="cpu") as f:
            for key in f.keys():
                if key.startswith("language_model."):
                    mapped = "model." + key[len("language_model."):]
                elif key.startswith(("vision_model.", "sam_model.", "projector.")) or key in {
                    "image_newline",
                    "view_seperator",
                }:
                    mapped = "model." + key
                else:
                    mapped = key
                state[mapped] = f.get_tensor(key)
        model.load_state_dict(state, strict=False)
        del state

        tokenizer = AutoTokenizer.from_pretrained(
            model_path,
            trust_remote_code=True,
            use_fast=False,
            fix_mistral_regex=True,
        )

        base_size = option_int(req, "base_size", 1024)
        image_size = option_int(req, "image_size", 640)
        patch_size = 16
        downsample_ratio = 4
        image_token_id = 128815
        image = ImageOps.exif_transpose(Image.open(image_path)).convert("RGB")
        transform = BasicImageTransform(mean=(0.5, 0.5, 0.5), std=(0.5, 0.5, 0.5), normalize=True)

        tokenized = []
        seq_mask = []
        tokenized_sep = text_encode(tokenizer, "", bos=False, eos=False)
        tokenized += tokenized_sep
        seq_mask += [False] * len(tokenized_sep)

        crop_mode = option_bool(req, "crop_mode", True)
        if not crop_mode:
            crop_ratio = [1, 1]
            crops = []
        elif image.size[0] <= image_size and image.size[1] <= image_size:
            crop_ratio = [1, 1]
            crops = []
        else:
            crops, crop_ratio = dynamic_preprocess(image, image_size=image_size)

        global_view = ImageOps.pad(
            image,
            (base_size, base_size),
            color=tuple(int(x * 255) for x in transform.mean),
        )
        images_list = [transform(global_view).to(torch.float16)]
        crop_list = [transform(crop).to(torch.float16) for crop in crops]
        width_crop_num, height_crop_num = crop_ratio

        num_queries_base = math.ceil((base_size // patch_size) / downsample_ratio)
        tokenized_image = ([image_token_id] * num_queries_base + [image_token_id]) * num_queries_base
        tokenized_image += [image_token_id]
        if width_crop_num > 1 or height_crop_num > 1:
            num_queries = math.ceil((image_size // patch_size) / downsample_ratio)
            tokenized_image += ([image_token_id] * (num_queries * width_crop_num) + [image_token_id]) * (
                num_queries * height_crop_num
            )
        tokenized += tokenized_image
        seq_mask += [True] * len(tokenized_image)

        tokenized_sep = text_encode(tokenizer, unlimited_prompt(req), bos=False, eos=False)
        tokenized += tokenized_sep
        seq_mask += [False] * len(tokenized_sep)
        tokenized = [0] + tokenized
        seq_mask = [False] + seq_mask

        input_ids = torch.LongTensor(tokenized).unsqueeze(0)
        images_ori = torch.stack(images_list, dim=0)
        images_crop = (
            torch.stack(crop_list, dim=0)
            if crop_list
            else torch.zeros((1, 3, base_size, base_size), dtype=torch.float16)
        )
        images_spatial_crop = torch.tensor([[width_crop_num, height_crop_num]], dtype=torch.long)
        images_seq_mask = torch.tensor(seq_mask, dtype=torch.bool).unsqueeze(0)

        if device == "mps":
            model = model.half().to(device)
            input_ids = input_ids.to(device)
            images_ori = images_ori.to(device)
            images_crop = images_crop.to(device)
            images_spatial_crop = images_spatial_crop.to(device)
            images_seq_mask = images_seq_mask.to(device)
        else:
            model = model.float()
            images_ori = images_ori.float()
            images_crop = images_crop.float()

        sliding_window = option_value(req, "sliding_window", None)
        if sliding_window is None or str(sliding_window).strip().lower() in {"", "none", "off", "false", "0"}:
            model.config.sliding_window = None
        else:
            model.config.sliding_window = int(sliding_window)

        default_max_new = int(os.environ.get("LLM_WIKI_UNLIMITED_OCR_MAX_NEW_TOKENS") or 4096)
        max_new_tokens = option_int(req, "max_new_tokens", option_int(req, "max_length", default_max_new))
        max_new_tokens = max(1, min(max_new_tokens, 32768))
        temperature = option_float(req, "temperature", 0.0)
        generation_kwargs = {}
        no_repeat_ngram_size = option_int(req, "no_repeat_ngram_size", 0)
        ngram_window = option_int(req, "ngram_window", 0)
        if no_repeat_ngram_size > 0 and ngram_window > 0:
            generation_kwargs["logits_processor"] = [
                SlidingWindowNoRepeatNgramProcessor(no_repeat_ngram_size, ngram_window)
            ]
        elif no_repeat_ngram_size > 0:
            generation_kwargs["no_repeat_ngram_size"] = no_repeat_ngram_size
        print(
            f"Running Unlimited-OCR compatibility runtime on {device}: "
            f"{len(tokenized)} input tokens, {sum(seq_mask)} image tokens",
            file=sys.stderr,
        )
        start = time.time()
        with torch.no_grad():
            output_ids = model.generate(
                input_ids=input_ids,
                images=[(images_crop, images_ori)],
                images_seq_mask=images_seq_mask,
                images_spatial_crop=images_spatial_crop,
                attention_mask=torch.ones_like(input_ids),
                do_sample=temperature > 0,
                temperature=temperature if temperature > 0 else None,
                eos_token_id=tokenizer.eos_token_id,
                pad_token_id=tokenizer.eos_token_id,
                max_new_tokens=max_new_tokens,
                use_cache=True,
                **generation_kwargs,
            )
        elapsed = time.time() - start
        output_tokens = output_ids[0, input_ids.shape[1]:].detach().cpu()
        text = tokenizer.decode(output_tokens, skip_special_tokens=False).strip()
        print(f"Unlimited-OCR compatibility runtime finished in {elapsed:.1f}s", file=sys.stderr)
        return text
    finally:
        sys.path = old_path
        if old_cuda is not None:
            torch.Tensor.cuda = old_cuda
        shutil.rmtree(package_root, ignore_errors=True)


def run_unlimited_ocr_mlx(req, image_path):
    width = int(req["width"])
    height = int(req["height"])
    model_path = req.get("model") or req.get("model_dir")
    if not model_path or not os.path.exists(model_path):
        raise RuntimeError("Unlimited-OCR-MLX model directory does not exist: " + str(model_path))

    torch_error = None
    try:
        text = run_unlimited_ocr_torch(req, image_path, model_path)
        text = clean_model_text(text)
        items = parse_unlimited_det_output(text, width, height)
        if not items:
            items = parse_coordinate_text(text, width, height)
        if not items:
            items = line_box_items(str(text), width, height)
        return items
    except Exception as exc:
        torch_error = exc
        print(f"Unlimited-OCR compatibility runtime failed, falling back to MLX: {exc}", file=sys.stderr)

    inference_py = os.path.join(model_path, "inference.py")
    if os.path.exists(inference_py):
        with open(inference_py, "r", encoding="utf-8") as f:
            source = f.read()
        patched = source.replace(
            "use_fast=False,\n    )",
            "use_fast=False,\n        fix_mistral_regex=True,\n    )",
        ).replace(
            "mx.ones((seq_len, seq_len), dtype=bool)",
            "mx.ones((seq_len, seq_len), dtype=mx.bool_)",
        ).replace(
            "mx.array([seq_mask], dtype=bool)",
            "mx.array([seq_mask], dtype=mx.bool_)",
        ).replace(
            "mx.array([seq_mask], dtype=mx.bool_)",
            "mx.array(seq_mask[None, :], dtype=mx.bool_)",
        ).replace(
            "np.zeros(len(input_ids) + total_image_feats, dtype=mx.bool_)",
            "np.zeros(len(input_ids) + total_image_feats, dtype=bool)",
        ).replace(
            """    # Reshape to [B, dim, src, src]
    x = pos_embed.transpose(0, 3, 1, 2)
    # Simple interpolation using reshape
    # MLX doesn't have native interpolate, use simple scaling
    x = x.reshape(B, dim, src * src)
    x = x.reshape(B, dim, target_size, target_size)
    x = x.transpose(0, 2, 3, 1)
    return x
""",
            """    if src == target_size:
        return pos_embed
    idx = mx.array([round(i * (src - 1) / max(target_size - 1, 1)) for i in range(target_size)], dtype=mx.int32)
    return mx.take(mx.take(pos_embed, idx, axis=1), idx, axis=2)
""",
        )
        if patched != source:
            with open(inference_py, "w", encoding="utf-8") as f:
                f.write(patched)

    model_py = os.path.join(model_path, "model.py")
    if os.path.exists(model_py):
        with open(model_py, "r", encoding="utf-8") as f:
            source = f.read()
        patched = source.replace(
            """    # Reshape to [B, dim, src, src]
    x = pos_embed.transpose(0, 3, 1, 2)
    # Simple interpolation using reshape
    # MLX doesn't have native interpolate, use simple scaling
    x = x.reshape(B, dim, src * src)
    x = x.reshape(B, dim, target_size, target_size)
    x = x.transpose(0, 2, 3, 1)
    return x
""",
            """    if src == target_size:
        return pos_embed
    idx = mx.array([round(i * (src - 1) / max(target_size - 1, 1)) for i in range(target_size)], dtype=mx.int32)
    return mx.take(mx.take(pos_embed, idx, axis=1), idx, axis=2)
""",
        ).replace(
            """                    # Scatter image features into positions where mask is True
                    inputs_embeds = inputs_embeds.at[idx].set(
                        mx.where(mask, img_feats, inputs_embeds[idx])
                    )
""",
            """                    # Image positions are inserted as one contiguous span after BOS.
                    image_start = 1
                    image_len = img_feats.shape[0]
                    updated = mx.concatenate(
                        [
                            inputs_embeds[idx, :image_start, :],
                            img_feats,
                            inputs_embeds[idx, image_start + image_len:, :],
                        ],
                        axis=0,
                    )
                    inputs_embeds = updated[None, :, :] if B == 1 else mx.concatenate(
                        [inputs_embeds[:idx], updated[None, :, :], inputs_embeds[idx + 1:]],
                        axis=0,
                    )
""",
        )
        if patched != source:
            with open(model_py, "w", encoding="utf-8") as f:
                f.write(patched)

    try:
        package_init = os.path.join(model_path, "__init__.py")
        if os.path.exists(package_init):
            spec = importlib.util.spec_from_file_location(
                "unlimited_ocr_mlx",
                package_init,
                submodule_search_locations=[model_path],
            )
            module = importlib.util.module_from_spec(spec)
            sys.modules["unlimited_ocr_mlx"] = module
            spec.loader.exec_module(module)
            UnlimitedOCRInference = module.UnlimitedOCRInference
        else:
            if model_path not in sys.path:
                sys.path.insert(0, model_path)
            from unlimited_ocr_mlx import UnlimitedOCRInference
    except Exception as exc:
        raise RuntimeError(
            "Unlimited-OCR-MLX requires the model repository code and dependencies: "
            "mlx, mlx-lm, safetensors, transformers, Pillow and numpy."
        ) from exc

    try:
        import mlx.nn as nn
        original_load_weights = nn.Module.load_weights

        def load_weights_compat(self, file_or_weights, strict=True):
            try:
                return original_load_weights(self, file_or_weights, strict=strict)
            except ValueError as exc:
                if strict and "parameters not in model" in str(exc):
                    if isinstance(file_or_weights, list):
                        from mlx.utils import tree_flatten

                        valid = {name for name, _ in tree_flatten(self.parameters())}
                        mapped = []
                        for name, value in file_or_weights:
                            if name.startswith("sam_model.neck."):
                                parts = name.split(".", 3)
                                if len(parts) == 4 and parts[2].isdigit():
                                    name = f"sam_model.neck.layers.{parts[2]}.{parts[3]}"
                            if name in valid:
                                if name.endswith(".weight") and len(value.shape) == 4:
                                    value = value.transpose(0, 2, 3, 1)
                                mapped.append((name, value))
                        return original_load_weights(self, mapped, strict=False)
                    return original_load_weights(self, file_or_weights, strict=False)
                raise

        nn.Module.load_weights = load_weights_compat
    except Exception:
        pass

    engine = UnlimitedOCRInference(model_path)
    with contextlib.redirect_stdout(sys.stderr):
        engine.load()
        text = infer_unlimited_ocr_mlx_original_style(
            engine,
            req,
            image_path,
            max_length=option_int(req, "max_length", option_int(req, "max_new_tokens", 32768)),
        )

    text = clean_model_text(text)
    items = parse_unlimited_det_output(text, width, height)
    if not items:
        items = parse_coordinate_text(text, width, height)
    if not items:
        if torch_error is not None:
            print(f"Unlimited-OCR MLX fallback produced unstructured text after PyTorch error: {torch_error}", file=sys.stderr)
        items = line_box_items(str(text), width, height)
    return items


def infer_unlimited_ocr_mlx_original_style(engine, req, image_path, max_length=32768):
    import mlx.core as mx

    tokenizer = engine.tokenizer
    model = engine.model
    base_size = option_int(req, "base_size", 1024)
    image_size = option_int(req, "image_size", 640)
    patch_size = 16
    downsample_ratio = 4
    image_token = "<image>"
    image_token_id = 128815

    image = ImageOps.exif_transpose(Image.open(image_path)).convert("RGB")
    w, h = image.size

    mean = np.array([0.5, 0.5, 0.5], dtype=np.float32)
    std = np.array([0.5, 0.5, 0.5], dtype=np.float32)

    def to_tensor(img):
        arr = np.array(img, dtype=np.float32) / 255.0
        arr = (arr - mean) / std
        return arr.transpose(2, 0, 1)

    prompt = "<image>" + unlimited_prompt(req)
    text_splits = prompt.split(image_token)
    tokenized = []
    seq_mask = []

    # Original "plain" conversation template emits only user content plus an empty assistant turn.
    tokenized_sep = tokenizer.encode(text_splits[0], add_special_tokens=False)
    tokenized += tokenized_sep
    seq_mask += [False] * len(tokenized_sep)

    crop_mode = option_bool(req, "crop_mode", True)
    if not crop_mode:
        crop_ratio = (1, 1)
        crop_images = []
    elif w <= image_size and h <= image_size:
        crop_ratio = (1, 1)
        crop_images = []
    else:
        crop_images, crop_ratio = dynamic_preprocess_unlimited(image, image_size=image_size)

    global_view = ImageOps.pad(image, (base_size, base_size), color=(127, 127, 127))
    orig_arr = to_tensor(global_view)[None, ...]

    width_crop_num, height_crop_num = crop_ratio
    if crop_images and (width_crop_num > 1 or height_crop_num > 1):
        patches_arr = np.stack([to_tensor(crop) for crop in crop_images], axis=0)
        patches_mx = mx.array(patches_arr)
    else:
        patches_mx = None

    num_queries_base = math.ceil((base_size // patch_size) / downsample_ratio)
    tokenized_image = ([image_token_id] * num_queries_base + [image_token_id]) * num_queries_base
    tokenized_image += [image_token_id]

    if patches_mx is not None:
        num_queries = math.ceil((image_size // patch_size) / downsample_ratio)
        tokenized_image += ([image_token_id] * (num_queries * width_crop_num) + [image_token_id]) * (
            num_queries * height_crop_num
        )

    tokenized += tokenized_image
    seq_mask += [True] * len(tokenized_image)

    tokenized_sep = tokenizer.encode(text_splits[-1], add_special_tokens=False)
    tokenized += tokenized_sep
    seq_mask += [False] * len(tokenized_sep)

    tokenized = [tokenizer.bos_token_id] + tokenized
    seq_mask = [False] + seq_mask

    input_ids = mx.array([tokenized], dtype=mx.int32)
    images_seq_mask = mx.array(np.array(seq_mask, dtype=bool)[None, :], dtype=mx.bool_)
    orig_mx = mx.array(orig_arr)

    print(f"Input: {len(tokenized)} tokens, {sum(seq_mask)} image tokens")
    print("Running OCR inference...")
    start = time.time()
    output_ids = model.generate(
        input_ids=input_ids,
        images=[(patches_mx, orig_mx)],
        images_seq_mask=images_seq_mask,
        images_spatial_crop=[crop_ratio],
        max_length=max_length,
        temperature=option_float(req, "temperature", 0.0),
        eos_token_id=tokenizer.eos_token_id,
    )
    elapsed = time.time() - start
    output_tokens = output_ids[0].tolist()[len(tokenized):]
    text = tokenizer.decode(output_tokens, skip_special_tokens=True).strip()
    print(f"\n=== OCR Result ({len(output_tokens)} tokens, {elapsed:.1f}s) ===")
    print(text)
    return text


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
        elif req.get("engine") == "unlimited-ocr-mlx":
            items = run_unlimited_ocr_mlx(req, temp_path)
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
