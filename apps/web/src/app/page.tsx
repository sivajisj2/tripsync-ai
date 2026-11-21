"use client";

import { useEffect, useState } from "react";

type Health = {
  service: string;
  status: string;
  version: string;
};

export default function Home() {
  const [health, setHealth] = useState<Health | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    const controller = new AbortController();

    fetch(`${process.env.NEXT_PUBLIC_API_URL}/health`, {
      signal: controller.signal,
    })
      .then((response) => {
        if (!response.ok) {
          throw new Error(`API returned ${response.status}`);
        }
        return response.json() as Promise<Health>;
      })
      .then(setHealth)
      .catch((err: unknown) => {
        if (!controller.signal.aborted) {
          setError(err instanceof Error ? err.message : "API unavailable");
        }
      });

    return () => controller.abort();
  }, []);

  return (
    <main className="min-h-screen bg-slate-950 text-white">
      <div className="mx-auto max-w-5xl px-6 py-20">
        <p className="mb-4 text-emerald-400">
          Beyond 9TO5 Buildathon
        </p>

        <h1 className="text-5xl font-bold">
          TripSync AI
        </h1>

        <p className="mt-5 text-lg text-slate-400">
          Plan together. Explore smarter. Travel better.
        </p>

        <div className="mt-12 rounded-2xl border border-slate-700 bg-slate-900 p-8">
          <h2 className="text-xl font-semibold">
            Rust Backend Status
          </h2>

          <p className="mt-4 text-emerald-400">
            {health
              ? `${health.service}: ${health.status}`
              : error
                ? `Connection failed: ${error}`
                : "Connecting to Rust API..."}
          </p>

          {health && (
            <p className="mt-2 text-sm text-slate-400">
              API version: {health.version}
            </p>
          )}
        </div>
      </div>
    </main>
  );
}