import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

const config: Config = {
  title: 'kmrs',
  tagline: 'Your comics, one static binary',
  favicon: 'img/favicon.svg',

  // Future flags, see https://docusaurus.io/docs/api/docusaurus-config#future
  future: {
    v4: true, // Improve compatibility with the upcoming Docusaurus v4
  },

  url: 'https://kmworks.github.io',
  baseUrl: '/kmrs/',

  organizationName: 'kmworks',
  projectName: 'kmrs',

  onBrokenLinks: 'throw',

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          editUrl: 'https://github.com/kmworks/kmrs/tree/master/website/',
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themes: [
    [
      '@easyops-cn/docusaurus-search-local',
      {
        hashed: true,
        indexBlog: false,
        docsRouteBasePath: '/docs',
      },
    ],
  ],

  themeConfig: {
    image: 'img/social-card.png',
    colorMode: {
      defaultMode: 'light',
      disableSwitch: false,
      respectPrefersColorScheme: true,
    },
    navbar: {
      title: 'kmrs',
      logo: {
        alt: 'kmrs',
        src: 'img/logo.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docs',
          position: 'left',
          label: 'Docs',
        },
        {
          href: 'https://github.com/kmworks/kmrs',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Docs',
          items: [
            {label: 'Introduction', to: '/docs'},
            {label: 'Installation', to: '/docs/installation'},
            {label: 'Configuration', to: '/docs/configuration'},
          ],
        },
        {
          title: 'Project',
          items: [
            {label: 'GitHub', href: 'https://github.com/kmworks/kmrs'},
            {label: 'Releases', href: 'https://github.com/kmworks/kmrs/releases'},
            {label: 'kmweb UI', href: 'https://github.com/kmworks/kmweb'},
            {label: 'KMReader', href: 'https://github.com/kmworks/kmreader'},
          ],
        },
        {
          title: 'Upstream',
          items: [
            {label: 'Komga', href: 'https://komga.org'},
            {label: 'Komga clients', href: 'https://komga.org/docs/category/readers'},
          ],
        },
      ],
      copyright: `kmrs is under the MIT License. Not affiliated with the komga project.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.duotoneDark,
      additionalLanguages: ['bash', 'toml', 'rust', 'nginx', 'json', 'yaml'],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
