FROM python:3.12-slim

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1

WORKDIR /app
COPY pyproject.toml README.md ./
COPY src ./src
RUN python -m pip install --no-cache-dir .

EXPOSE 8081
CMD ["uvicorn", "nexus_core.main:app", "--host", "0.0.0.0", "--port", "8081"]
