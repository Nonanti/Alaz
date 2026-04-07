FROM python:3.11-slim

WORKDIR /app

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY main.py .

# Pre-download model at build time
RUN python -c "from transformers import AutoModel; AutoModel.from_pretrained('jinaai/jina-colbert-v2', trust_remote_code=True)"

EXPOSE 80

CMD ["uvicorn", "main:app", "--host", "0.0.0.0", "--port", "80", "--log-level", "info"]
