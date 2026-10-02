import { useCallback, useEffect, useMemo, useState } from "react";

type BrowserLocation = {
  pathname: string;
  search: string;
  hash: string;
};

type HistoryTarget = {
  pathname?: string;
  search?: string;
  hash?: string;
};

function readLocation(): BrowserLocation {
  if (typeof window === "undefined") {
    return {
      pathname: "/",
      search: "",
      hash: "",
    };
  }

  return {
    pathname: window.location.pathname,
    search: window.location.search,
    hash: window.location.hash,
  };
}

export function useLocation(): BrowserLocation {
  const [location, setLocation] =
    useState<BrowserLocation>(readLocation);

  useEffect(() => {
    const update = () => setLocation(readLocation());

    window.addEventListener("popstate", update);
    window.addEventListener("hashchange", update);

    return () => {
      window.removeEventListener("popstate", update);
      window.removeEventListener("hashchange", update);
    };
  }, []);

  return location;
}

export function useHistory() {
  const replace = useCallback((target: HistoryTarget) => {
    if (typeof window === "undefined") {
      return;
    }

    const url = new URL(window.location.href);

    if (target.pathname !== undefined) {
      url.pathname = target.pathname;
    }

    if (target.search !== undefined) {
      url.search = target.search;
    }

    if (target.hash !== undefined) {
      url.hash = target.hash;
    }

    window.history.replaceState(
      window.history.state,
      "",
      url,
    );

    window.dispatchEvent(new PopStateEvent("popstate"));
  }, []);

  return useMemo(
    () => ({
      replace,
    }),
    [replace],
  );
}

export function Redirect({
  to,
}: {
  to: string;
}) {
  useEffect(() => {
    window.location.replace(to);
  }, [to]);

  return null;
}

