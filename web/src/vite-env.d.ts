/// <reference types="vite/client" />

declare global {
  interface Window {
    __PLANTOOL_TOKEN__?: string;
  }
}

export {};
