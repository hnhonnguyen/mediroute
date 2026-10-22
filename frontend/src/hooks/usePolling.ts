import { useCallback, useEffect, useRef, useState } from 'react';

interface PollingResult<T> {
  data: T | null;
  error: string | null;
  loading: boolean;
  refresh: () => Promise<void>;
}

export function usePolling<T>(
  fetcher: () => Promise<T>,
  intervalMs = 1000,
  enabled = true,
): PollingResult<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const fetcherRef = useRef(fetcher);
  fetcherRef.current = fetcher;

  const refresh = useCallback(async () => {
    try {
      const result = await fetcherRef.current();
      setData(result);
      setError(null);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : 'Could not reach the server');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!enabled) {
      return;
    }
    let cancelled = false;
    let timer: number | undefined;

    const tick = async () => {
      await refresh();
      if (!cancelled) {
        timer = window.setTimeout(tick, intervalMs);
      }
    };
    void tick();

    return () => {
      cancelled = true;
      if (timer !== undefined) {
        window.clearTimeout(timer);
      }
    };
  }, [enabled, intervalMs, refresh]);

  return { data, error, loading, refresh };
}
