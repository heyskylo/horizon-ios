"use client";

import { t } from "@/lib/i18n";
import styles from "./page.module.css";

const WAYS = [
  { name: "landing.way.game", how: "F3+O" },
  { name: "landing.way.ctl", how: "horizon-ctl profile start | stop" },
  { name: "landing.way.server", how: "horizon-report 60" },
];

const READS = ["tick", "functions", "slow", "compare"];

interface Props {
  ready: boolean;
  dragging: boolean;
  problem: string | null;
  onChoose: () => void;
}

/** What someone sees before a capture is open: what this is, how to get a capture, and the way in. */
export function Landing({ ready, dragging, problem, onChoose }: Props) {
  return (
    <>
      <section className={dragging ? styles.landingActive : styles.landing}>
        <h1 className={styles.display}>{t("landing.title")}</h1>
        <p className={styles.lede}>{t("landing.lede")}</p>
        {problem ? <p className={styles.error}>{problem}</p> : null}
        <div className={styles.actions}>
          <button type="button" className={styles.button} onClick={onChoose} disabled={!ready}>
            {ready ? t("app.choose") : t("app.loadingReader")}
          </button>
          <span className={styles.muted}>{t("app.dropHint")}</span>
        </div>
        <p className={styles.note}>{t("app.privacy")}</p>

        <div className={styles.landingLists}>
          <section>
            <h2 className={styles.landingHeading}>{t("landing.get")}</h2>
            <ul className={styles.landingList}>
              {WAYS.map(({ name, how }) => (
                <li key={name} className={styles.landingRow}>
                  <span>{t(name)}</span>
                  <code className={styles.landingCode}>{how}</code>
                </li>
              ))}
            </ul>
            <p className={styles.note}>{t("landing.where")}</p>
          </section>
          <section>
            <h2 className={styles.landingHeading}>{t("landing.reads")}</h2>
            <ul className={styles.landingList}>
              {READS.map((id) => (
                <li key={id} className={styles.landingRow}>
                  <span>{t(`landing.read.${id}`)}</span>
                  <span className={styles.muted}>{t(`landing.read.${id}.note`)}</span>
                </li>
              ))}
            </ul>
          </section>
        </div>
      </section>
      <div className={styles.wordmark} aria-hidden="true">
        Horizon
      </div>
    </>
  );
}
