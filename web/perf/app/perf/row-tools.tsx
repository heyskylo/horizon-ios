"use client";

import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { copyText } from "@/lib/clipboard";
import { has, t } from "@/lib/i18n";
import { infoKey, isId } from "@/lib/info";
import { placeTip } from "@/lib/tip-place";
import type { TipPlace } from "@/lib/tip-place";
import { Icons } from "./icons";
import styles from "./page.module.css";

const CLOSE_DELAY_MS = 120;
const FEEDBACK_MS = 1500;

type CopyState = "idle" | "done" | "failed";

/** What a well-known span or function does, in the player's language; null when the id has no entry. */
export function infoText(id: string | null | undefined): string | null {
  return isId(id) && has(infoKey(id)) ? t(infoKey(id)) : null;
}

function Tip({ anchor, id, title, text, onEnter, onLeave }: { anchor: HTMLElement; id: string; title: string | null; text: string; onEnter: () => void; onLeave: () => void }) {
  const box = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<TipPlace | null>(null);

  useLayoutEffect(() => {
    const tip = box.current;
    if (!tip) return;
    const a = anchor.getBoundingClientRect();
    const size = tip.getBoundingClientRect();
    const view = { width: document.documentElement.clientWidth, height: document.documentElement.clientHeight };
    setPlace(placeTip({ left: a.left, top: a.top, width: a.width, height: a.height }, { width: size.width, height: size.height }, view));
  }, [anchor, text]);

  const root = anchor.closest("[data-tip-root]") ?? document.body;
  return createPortal(
    <div
      ref={box}
      id={id}
      role="tooltip"
      className={styles.tip}
      style={place ? { left: place.left, top: place.top } : { visibility: "hidden" }}
      onPointerEnter={onEnter}
      onPointerLeave={onLeave}
    >
      {title ? <strong className={styles.tipTitle}>{title}</strong> : null}
      {text}
    </div>,
    root,
  );
}

/** A small marker after a well-known name; hover or focus explains it. */
function InfoPoint({ name, title, text }: { name: string; title: string | null; text: string }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const timer = useRef(0);
  const pointer = useRef("key");
  const tipId = useId();

  const show = () => {
    window.clearTimeout(timer.current);
    setOpen(true);
  };
  const hideSoon = () => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setOpen(false), CLOSE_DELAY_MS);
  };

  useEffect(() => () => window.clearTimeout(timer.current), []);

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    document.addEventListener("keydown", onKey);
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    return () => {
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
    };
  }, [open]);

  return (
    <>
      <button
        ref={anchor}
        type="button"
        className={styles.info}
        aria-label={t("info.label", { name })}
        aria-describedby={open ? tipId : undefined}
        onPointerDown={(event) => {
          pointer.current = event.pointerType;
        }}
        onPointerEnter={(event) => event.pointerType === "mouse" && show()}
        onPointerLeave={(event) => event.pointerType === "mouse" && hideSoon()}
        onFocus={() => pointer.current === "key" && show()}
        onBlur={() => {
          pointer.current = "key";
          setOpen(false);
        }}
        onClick={() => {
          if (pointer.current === "key") show();
          else if (pointer.current !== "mouse") setOpen((value) => !value);
        }}
      >
        <Icons.info width={13} height={13} />
      </button>
      {open && anchor.current ? <Tip anchor={anchor.current} id={tipId} title={title} text={text} onEnter={show} onLeave={hideSoon} /> : null}
    </>
  );
}

/** Copies a row's stable id; shown on row hover or keyboard focus, so rows stay clean at rest. */
function CopyId({ id }: { id: string }) {
  const [state, setState] = useState<CopyState>("idle");
  const timer = useRef(0);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const copy = async () => {
    const ok = await copyText(id);
    setState(ok ? "done" : "failed");
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setState("idle"), FEEDBACK_MS);
  };

  const note = state === "idle" ? "" : t(state === "done" ? "copy.done" : "copy.failed");
  return (
    <>
      <button type="button" className={styles.copyId} data-state={state} aria-label={t("copy.label", { id })} title={id} onClick={copy}>
        {note ? <span className={styles.copyNote}>{note}</span> : null}
        {state === "done" ? <Icons.check width={13} height={13} /> : <Icons.copy width={13} height={13} />}
      </button>
      <span className={styles.srOnly} role="status">
        {note}
      </span>
    </>
  );
}

/** The marker and copy control of a row, at the end of its name cell; `about` and `subject` name what a chained row's marker explains. */
export function RowTools({ id, name, about = id, subject }: { id: string | null | undefined; name: string; about?: string | null; subject?: string }) {
  if (!isId(id)) return null;
  const text = infoText(about);
  return (
    <>
      {text ? <InfoPoint name={subject ?? name} title={subject ?? null} text={text} /> : null}
      <CopyId id={id} />
    </>
  );
}
