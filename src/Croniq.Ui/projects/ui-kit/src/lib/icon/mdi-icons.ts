export type MdiIconDefinition = {
  viewBox: string;
  body: string;
};

const DEFAULT_VIEW_BOX = '0 0 24 24';

export const MDI_ICONS = {
  magnify: {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M9.5 3A6.5 6.5 0 0 1 16 9.5c0 1.61-.59 3.09-1.56 4.23l.27.27h.79l5 5l-1.5 1.5l-5-5v-.79l-.27-.27A6.52 6.52 0 0 1 9.5 16A6.5 6.5 0 0 1 3 9.5A6.5 6.5 0 0 1 9.5 3m0 2C7 5 5 7 5 9.5S7 14 9.5 14S14 12 14 9.5S12 5 9.5 5"/>',
  },
  refresh: {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M17.65 6.35A7.96 7.96 0 0 0 12 4a8 8 0 0 0-8 8a8 8 0 0 0 8 8c3.73 0 6.84-2.55 7.73-6h-2.08A5.99 5.99 0 0 1 12 18a6 6 0 0 1-6-6a6 6 0 0 1 6-6c1.66 0 3.14.69 4.22 1.78L13 11h7V4z"/>',
  },
  'rotate-360': {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M12 7C6.5 7 2 9.2 2 12c0 2.2 2.9 4.1 7 4.8V20l4-4l-4-4v2.7c-3.2-.6-5-1.9-5-2.7c0-1.1 3-3 8-3s8 1.9 8 3c0 .7-1.5 1.9-4 2.5v2.1c3.5-.8 6-2.5 6-4.6c0-2.8-4.5-5-10-5"/>',
  },
  plus: {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6z"/>',
  },
  pencil: {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M20.71 7.04c.39-.39.39-1.04 0-1.41l-2.34-2.34c-.37-.39-1.02-.39-1.41 0l-1.84 1.83l3.75 3.75M3 17.25V21h3.75L17.81 9.93l-3.75-3.75z"/>',
  },
  'trash-can-outline': {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M9 3v1H4v2h1v13a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V6h1V4h-5V3zM7 6h10v13H7zm2 2v9h2V8zm4 0v9h2V8z"/>',
  },
  'chevron-left': {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M15.41 16.58L10.83 12l4.58-4.59L14 6l-6 6l6 6z"/>',
  },
  'chevron-right': {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M8.59 16.58L13.17 12L8.59 7.41L10 6l6 6l-6 6z"/>',
  },
  'chevron-up': {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M7.41 15.41L12 10.83l4.59 4.58L18 14l-6-6l-6 6z"/>',
  },
  'chevron-down': {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M7.41 8.58L12 13.17l4.59-4.59L18 10l-6 6l-6-6z"/>',
  },
  close: {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M19 6.41L17.59 5L12 10.59L6.41 5L5 6.41L10.59 12L5 17.59L6.41 19L12 13.41L17.59 19L19 17.59L13.41 12z"/>',
  },
  check: {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M21 7L9 19l-5.5-5.5l1.41-1.41L9 16.17L19.59 5.59z"/>',
  },
  'alert-outline': {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M12 2L1 21h22M12 6l7.53 13H4.47M11 10v4h2v-4m-2 6v2h2v-2"/>',
  },
  'pause-circle-outline': {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M12 20a8 8 0 0 1-8-8a8 8 0 0 1 8-8a8 8 0 0 1 8 8a8 8 0 0 1-8 8m0-18a10 10 0 0 0-10 10a10 10 0 0 0 10 10a10 10 0 0 0 10-10a10 10 0 0 0-10-10M9 9h2v6H9m4-6h2v6h-2"/>',
  },
  'information-outline': {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M11 9h2V7h-2m1 13c-4.41 0-8-3.59-8-8s3.59-8 8-8s8 3.59 8 8s-3.59 8-8 8m0-18A10 10 0 0 0 2 12a10 10 0 0 0 10 10a10 10 0 0 0 10-10A10 10 0 0 0 12 2m-1 15h2v-6h-2z"/>',
  },
  'content-copy': {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M19 21H8V7h11m0-2H8a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2m-3-4H4a2 2 0 0 0-2 2v14h2V3h12z"/>',
  },
  'filter-remove': {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="M14.76 20.83L17.6 18l-2.84-2.83l1.41-1.41L19 16.57l2.83-2.81l1.41 1.41L20.43 18l2.81 2.83l-1.41 1.41L19 19.4l-2.83 2.84zM12 12v7.88c.04.3-.06.62-.29.83a.996.996 0 0 1-1.41 0L8.29 18.7a.99.99 0 0 1-.29-.83V12h-.03L2.21 4.62a1 1 0 0 1 .17-1.4c.19-.14.4-.22.62-.22h14c.22 0 .43.08.62.22a1 1 0 0 1 .17 1.4L12.03 12z"/>',
  },
  rocket: {
    viewBox: DEFAULT_VIEW_BOX,
    body: '<path fill="currentColor" d="m13.16 22.19l-1.66-3.84c1.6-.58 3.07-1.35 4.43-2.27l-2.78 6.11m-7.5-9.69l-3.84-1.65l6.11-2.78a20 20 0 0 0-2.27 4.43M21.66 2.35S23.78 7.31 18.11 13c-2.2 2.17-4.58 3.5-6.73 4.34c-.74.28-1.57.1-2.12-.46l-2.13-2.13c-.56-.56-.74-1.38-.47-2.13C7.5 10.5 8.83 8.09 11 5.89C16.69.216 21.66 2.35 21.66 2.35M6.25 22H4.84l4.09-4.1c.3.21.63.36.97.45zM2 22v-1.41l4.77-4.78l1.43 1.42L3.41 22zm0-2.84v-1.41l3.65-3.65c.09.35.24.68.45.97zM16 6a2 2 0 1 0 0 4c1.11 0 2-.89 2-2a2 2 0 0 0-2-2"/>'
  },
} as const satisfies Record<string, MdiIconDefinition>;

export type MdiIconName = keyof typeof MDI_ICONS;
