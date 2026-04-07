"""Alaz ColBERT Server — Jina-ColBERT-v2 token-level embeddings via FastAPI."""

import logging
import os
import glob
from contextlib import asynccontextmanager

import torch
import torch.nn as nn
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
from safetensors.torch import load_file
from transformers import AutoModel

logger = logging.getLogger("colbert-server")

model = None
tokenizer = None
projection = None

COLBERT_DIM = 128


@asynccontextmanager
async def lifespan(app: FastAPI):
    global model, tokenizer, projection
    logger.info("Loading Jina-ColBERT-v2 model...")

    model = AutoModel.from_pretrained("jinaai/jina-colbert-v2", trust_remote_code=True)
    tokenizer = model.tokenizer
    model.eval()

    # Load the trained linear projection (1024 -> 128) from checkpoint
    # This layer is marked "UNEXPECTED" by transformers but exists in safetensors
    cache_dir = os.path.expanduser("~/.cache/huggingface/hub/")
    snap_dirs = glob.glob(cache_dir + "models--jinaai--jina-colbert-v2/snapshots/*")

    projection = nn.Linear(model.config.hidden_size, COLBERT_DIM, bias=False)

    if snap_dirs:
        safetensor_files = glob.glob(os.path.join(snap_dirs[0], "*.safetensors"))
        for sf in safetensor_files:
            state = load_file(sf)
            if "linear.weight" in state:
                projection.weight = nn.Parameter(state["linear.weight"])
                logger.info(f"Loaded trained projection: {state['linear.weight'].shape}")
                break

    # Match projection dtype to model dtype
    model_dtype = next(model.parameters()).dtype
    projection = projection.to(dtype=model_dtype)
    projection.eval()
    logger.info(f"Model ready: hidden={model.config.hidden_size} -> colbert={COLBERT_DIM}, dtype={model_dtype}")
    yield


app = FastAPI(title="Alaz ColBERT Server", lifespan=lifespan)


class EmbedRequest(BaseModel):
    texts: list[str]
    is_query: bool = False


class EmbedResponse(BaseModel):
    embeddings: list[list[list[float]]]


@app.post("/embed", response_model=EmbedResponse)
def embed(request: EmbedRequest):
    if not request.texts:
        return EmbedResponse(embeddings=[])

    try:
        inputs = tokenizer(
            request.texts,
            padding=True,
            truncation=True,
            return_tensors="pt",
            max_length=512,
        )

        with torch.no_grad():
            outputs = model(**inputs)
            # last_hidden_state: (batch, seq_len, 1024)
            hidden = outputs.last_hidden_state
            # Trained projection: (batch, seq_len, 1024) -> (batch, seq_len, 128)
            token_embs = projection(hidden)
            # Cast to float32 and L2 normalize each token vector
            token_embs = token_embs.float()
            token_embs = torch.nn.functional.normalize(token_embs, p=2, dim=-1)

        attention_mask = inputs["attention_mask"]  # (batch, seq_len)
        result = []
        for i in range(token_embs.shape[0]):
            mask = attention_mask[i].bool()
            tokens = token_embs[i][mask].cpu().tolist()
            result.append(tokens)

        return EmbedResponse(embeddings=result)

    except Exception as e:
        logger.exception("Embedding failed")
        raise HTTPException(status_code=500, detail=str(e))


@app.get("/health")
def health():
    return {"status": "ok", "model": "jina-colbert-v2", "dim": COLBERT_DIM}
