import type {ReactNode} from 'react';
import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import CodeBlock from '@theme/CodeBlock';
import Tabs from '@theme/Tabs';
import TabItem from '@theme/TabItem';
import {
  ArrowRight,
  BookOpen,
  Database,
  DeviceMobile,
  Feather,
  GithubLogo,
  GitDiff,
  Pulse,
  Translate,
} from '@phosphor-icons/react';

import styles from './index.module.css';

const dockerRun = `docker run -d \\
  --name=komga \\
  --user 1000:1000 \\
  -p 25600:25600 \\
  --mount type=bind,source=/path/to/config,target=/config \\
  --mount type=bind,source=/path/to/data,target=/data \\
  --restart unless-stopped \\
  ghcr.io/kmworks/kmrs`;

const binaryRun = `# grab the archive for your platform from the releases page:
#   https://github.com/kmworks/kmrs/releases/latest
./kmrs   # serves on http://localhost:25600`;

function Hero(): ReactNode {
  return (
    <header className={styles.hero}>
      <div className={styles.heroBg} />
      <div className="container">
        <div className={styles.heroInner}>
          <div>
            <p className={`${styles.eyebrow} ${styles.rise} ${styles.d1}`}>
              Komga-compatible, drop-in
            </p>
            <h1 className={`${styles.heroTitle} ${styles.rise} ${styles.d2}`}>
              Your comics,
              <br />
              <em>one</em> static binary.
            </h1>
            <p className={`${styles.heroSub} ${styles.rise} ${styles.d3}`}>
              Serve your comic and manga library to the web, your e-reader,
              and every app in between.
            </p>
            <div className={`${styles.ctaRow} ${styles.rise} ${styles.d4}`}>
              <Link className={styles.btnPrimary} to="/docs/installation">
                Get started <ArrowRight size={16} weight="bold" />
              </Link>
              <Link
                className={styles.btnGhost}
                href="https://github.com/kmworks/kmrs">
                <GithubLogo size={17} /> GitHub
              </Link>
            </div>
          </div>
          <div className={`${styles.heroCode} ${styles.rise} ${styles.d5}`}>
            <div className={styles.codeCaption}>
              <span>terminal</span>
              <span>one command</span>
            </div>
            <CodeBlock language="bash">{dockerRun}</CodeBlock>
          </div>
        </div>
      </div>
    </header>
  );
}

function Bento(): ReactNode {
  return (
    <section className={styles.section}>
      <div className="container">
        <div className={`${styles.sectionHead} ${styles.reveal}`}>
          <h2 className={styles.sectionTitle}>Same server, smaller footprint</h2>
          <p className={styles.sectionSub}>
            Same port, same mounts, same env vars, same database. What leaves
            is the runtime weight.
          </p>
        </div>
        <div className={styles.bento}>
          <div className={`${styles.cell} ${styles.cellHalf} ${styles.reveal}`}>
            <div className={styles.cellIcon}>
              <GitDiff size={26} weight="duotone" />
            </div>
            <h3 className={styles.cellTitle}>API parity, verified</h3>
            <p className={styles.cellBody}>
              REST, OPDS v1.2/v2, SSE, Kobo and KOReader sync. A differential
              harness compares ~105 endpoints against a live Java instance of
              komga 1.27.1.
            </p>
            <CodeBlock language="bash">
              {`python3 tests/diff/diff.py \\
  --java-jar komga.jar \\
  --rust-bin ./kmrs`}
            </CodeBlock>
          </div>
          <div className={`${styles.cell} ${styles.cellHalf} ${styles.reveal}`}>
            <div className={styles.cellIcon}>
              <Database size={26} weight="duotone" />
            </div>
            <h3 className={styles.cellTitle}>Your data, as-is</h3>
            <p className={styles.cellBody}>
              Point kmrs at an existing komga data directory and it upgrades
              <code>database.sqlite</code> in place with byte-for-byte Flyway
              migrations. The Java version can still open libraries written by
              kmrs.
            </p>
          </div>
          <div className={`${styles.cell} ${styles.cellThird} ${styles.reveal}`}>
            <div className={styles.cellIcon}>
              <Feather size={26} weight="duotone" />
            </div>
            <h3 className={styles.cellTitle}>Runs anywhere</h3>
            <p className={styles.cellBody}>
              One static binary for Linux, macOS, and Windows, x86_64 and
              aarch64.
            </p>
          </div>
          <div
            className={`${styles.cell} ${styles.cellThird} ${styles.cellCjk} ${styles.reveal}`}>
            <div className={styles.cellIcon}>
              <Translate size={26} weight="duotone" />
            </div>
            <h3 className={styles.cellTitle}>CJK cross-search</h3>
            <p className={styles.cellBody}>
              Simplified and traditional Chinese match each other, and CJK
              unigrams find titles mid-run.
            </p>
            <span className={styles.cjkGlyphs}>简繁</span>
          </div>
          <div className={`${styles.cell} ${styles.cellThird} ${styles.reveal}`}>
            <div className={styles.cellIcon}>
              <Pulse size={26} weight="duotone" />
            </div>
            <h3 className={styles.cellTitle}>Heap profiling built in</h3>
            <p className={styles.cellBody}>
              jemalloc sampling with a pprof endpoint, always on in release
              builds.
            </p>
          </div>
        </div>
      </div>
    </section>
  );
}

function Quickstart(): ReactNode {
  return (
    <section className={styles.section}>
      <div className="container">
        <div className={styles.split}>
          <div className={styles.reveal}>
            <h2 className={styles.sectionTitle}>Up and reading in one command</h2>
            <div className={styles.steps}>
              <div className={styles.step}>
                <span className={styles.stepName}>Install</span>
                <p className={styles.stepDesc}>
                  One <code>docker run</code>, or a single static binary from
                  the releases page.
                </p>
              </div>
              <div className={styles.step}>
                <span className={styles.stepName}>Keep your library</span>
                <p className={styles.stepDesc}>
                  Point <code>/config</code> at your existing komga data. kmrs
                  upgrades it in place.
                </p>
              </div>
              <div className={styles.step}>
                <span className={styles.stepName}>Pick a reader</span>
                <p className={styles.stepDesc}>
                  The bundled kmweb UI, KMReader, KOReader, Kobo, or any
                  OPDS client.
                </p>
              </div>
            </div>
          </div>
          <div className={`${styles.codePanel} ${styles.reveal}`}>
            <Tabs>
              <TabItem value="docker" label="Docker" default>
                <CodeBlock language="bash">{dockerRun}</CodeBlock>
              </TabItem>
              <TabItem value="binary" label="Binary">
                <CodeBlock language="bash">{binaryRun}</CodeBlock>
              </TabItem>
            </Tabs>
          </div>
        </div>
      </div>
    </section>
  );
}

function Clients(): ReactNode {
  const clients = [
    {
      label: 'KMReader',
      href: 'https://github.com/kmworks/kmreader',
      icon: <DeviceMobile size={18} />,
      desc: 'iOS / macOS / tvOS',
    },
    {
      label: 'kmweb',
      href: 'https://github.com/kmworks/kmweb',
      icon: <BookOpen size={18} />,
      desc: 'bundled web UI',
    },
    {
      label: 'KOReader',
      href: 'https://koreader.rocks',
      icon: <BookOpen size={18} />,
      desc: 'progress sync',
    },
    {
      label: 'Kobo',
      href: 'https://komga.org/docs/guides/kobo',
      icon: <BookOpen size={18} />,
      desc: 'native sync',
    },
    {
      label: 'Any Komga client',
      href: 'https://komga.org/docs/category/readers',
      icon: <ArrowRight size={18} weight="bold" />,
      desc: 'OPDS and REST',
    },
  ];
  return (
    <section className={`${styles.section} ${styles.clients}`}>
      <div className="container">
        <div className={styles.reveal}>
          <h2 className={styles.sectionTitle}>Bring your own reader</h2>
          <p className={styles.sectionSub}>
            Compatible with the Komga API and its reader ecosystem.
          </p>
          <div className={styles.chipRow}>
            {clients.map((c) => (
              <Link className={styles.chip} href={c.href} key={c.label}>
                {c.icon}
                {c.label}
              </Link>
            ))}
          </div>
        </div>
      </div>
    </section>
  );
}

function FinalCta(): ReactNode {
  return (
    <section className={styles.final}>
      <div className={styles.finalBg} />
      <div className={`container ${styles.reveal}`}>
        <h2 className={styles.finalTitle}>Start serving your comics.</h2>
        <div className={styles.ctaRow}>
          <Link className={styles.btnPrimary} to="/docs/installation">
            Get started <ArrowRight size={16} weight="bold" />
          </Link>
          <Link className={styles.btnGhost} to="/docs">
            Read the docs
          </Link>
        </div>
      </div>
    </section>
  );
}

export default function Home(): ReactNode {
  return (
    <Layout
      title="Comic & manga server in a single binary"
      description="kmrs is a comic and manga server in a single static Rust binary, drop-in compatible with Komga: same API, same database.">
      <div className={styles.page}>
        <Hero />
        <Bento />
        <Quickstart />
        <Clients />
        <FinalCta />
      </div>
    </Layout>
  );
}
