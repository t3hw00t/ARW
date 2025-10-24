import json
import sys
import traceback


def emit(payload: dict) -> None:
    json.dump(payload, sys.stdout, ensure_ascii=False)
    sys.stdout.flush()


def main() -> None:
    try:
        request = json.load(sys.stdin)
    except Exception as exc:  # noqa: BLE001
        emit({"ok": False, "error": "decode", "detail": str(exc)})
        return

    text = request.get("text") or ""
    budget = request.get("budget") or {}
    ratio = budget.get("ratio")
    target = budget.get("target_tokens")
    mode = (request.get("mode") or "extractive").lower()
    extras = request.get("extras") or {}

    try:
        from llmlingua import PromptCompressor  # type: ignore
    except Exception as exc:  # noqa: BLE001
        emit({"ok": False, "error": "import", "detail": str(exc)})
        return

    compressor_kwargs = {}
    config = extras.get("config")
    if isinstance(config, dict):
        compressor_kwargs.update(config)

    try:
        compressor = PromptCompressor(**compressor_kwargs)
    except TypeError:
        compressor = PromptCompressor()
    except Exception as exc:  # noqa: BLE001
        emit({"ok": False, "error": "init", "detail": str(exc)})
        return

    call_kwargs = extras.get("call_kwargs")
    if not isinstance(call_kwargs, dict):
        call_kwargs = {}
    if ratio is not None:
        call_kwargs.setdefault("ratio", ratio)
    if target is not None:
        try:
            call_kwargs.setdefault("target_token", int(target))
        except Exception:  # noqa: BLE001
            call_kwargs.setdefault("target_token", target)
    if mode:
        call_kwargs.setdefault("mode", mode)

    try:
        result = compressor.compress_prompt(text, **call_kwargs)
    except Exception as exc:  # noqa: BLE001
        emit(
            {
                "ok": False,
                "error": "compress",
                "detail": str(exc),
                "trace": traceback.format_exc(),
            }
        )
        return

    if isinstance(result, dict):
        compressed = (
            result.get("compressed_prompt")
            or result.get("compressed_text")
            or result.get("prompt")
            or ""
        )
        meta = {
            "kept_spans": result.get("kept_spans")
            or result.get("keep_sentences")
            or result.get("keep_ids"),
            "ratio": result.get("ratio")
            or result.get("compression_rate")
            or result.get("origin_compression_rate"),
            "raw": result,
        }
        emit({"ok": True, "compressed_text": compressed, "meta": meta})
        return

    if isinstance(result, str):
        emit({"ok": True, "compressed_text": result, "meta": {"ratio": ratio}})
        return

    emit(
        {
            "ok": False,
            "error": "unexpected_result",
            "detail": f"unsupported type: {type(result).__name__}",
        }
    )


if __name__ == "__main__":
    main()
