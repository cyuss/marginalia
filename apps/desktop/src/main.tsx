import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import "./styles.css";

/**
 * TanStack Query handles async state. WHY no global store yet: there is no
 * genuine global UI state in Phase 0, and adding Zustand "because we will need
 * it" is how a store becomes a dumping ground.
 */
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Local-first: data comes from our own SQLite, so refetching on every
      // window focus is noise rather than freshness.
      refetchOnWindowFocus: false,
      retry: 1,
    },
  },
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </React.StrictMode>,
);
