import { useEffect, useRef, useState } from "react";

/** Save feedback belongs to its originating page, including asynchronous completions. */
export function usePageConfirmation(page: string) {
  const currentPage = useRef(page);
  const generation = useRef(0);
  if (currentPage.current !== page) {
    currentPage.current = page;
    generation.current += 1;
  }
  const origin = generation.current;
  const [notice, setNotice] = useState<{ text: string; generation: number } | null>(null);
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 4000);
    return () => window.clearTimeout(timer);
  }, [notice]);
  return {
    message: notice?.generation === origin ? notice.text : null,
    show(text: string) {
      if (generation.current === origin) setNotice({ text, generation: origin });
    },
  };
}
