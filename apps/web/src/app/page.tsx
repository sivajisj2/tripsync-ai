"use client";

import { useEffect, useState } from "react";

type Place = {
  id: string;
  name: string;
  address: string | null;
  rating: number | null;
  maps_url: string | null;
};

export default function Home() {
  const [city, setCity] = useState("Mysuru Karnataka");
  const [category, setCategory] = useState("tourist attractions");
  const [places, setPlaces] = useState<Place[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  async function searchPlaces() {
    setLoading(true);
    setError("");

    try {
      const params = new URLSearchParams({ city, category });
      const response = await fetch(
        `${process.env.NEXT_PUBLIC_API_URL}/api/places/search?${params}`
      );

      if (!response.ok) {
        throw new Error(`Search failed: HTTP ${response.status}`);
      }

      setPlaces((await response.json()) as Place[]);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unknown error");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    // Search is user-triggered to conserve sponsor credits.
  }, []);

  return (
    <main className="min-h-screen bg-slate-950 px-6 py-12 text-white">
      <div className="mx-auto max-w-6xl">
        <p className="text-emerald-400">
          SerpApi + Nexconn + Rust
        </p>

        <h1 className="mt-3 text-5xl font-bold">
          TripSync AI
        </h1>

        <p className="mt-4 text-slate-400">
          Discover real places. Plan together.
        </p>

        <div className="mt-10 flex flex-wrap gap-3">
          <input
            value={city}
            onChange={(e) => setCity(e.target.value)}
            placeholder="Enter destination"
            className="min-w-60 flex-1 rounded-xl border border-slate-700 bg-slate-900 p-3"
          />

          <select
            value={category}
            onChange={(e) => setCategory(e.target.value)}
            className="rounded-xl border border-slate-700 bg-slate-900 p-3"
          >
            <option value="tourist attractions">Attractions</option>
            <option value="restaurants">Restaurants</option>
            <option value="hotels">Hotels</option>
            <option value="cafes">Cafes</option>
          </select>

          <button
            onClick={searchPlaces}
            disabled={loading || !city.trim()}
            className="rounded-xl bg-emerald-500 px-6 py-3 font-semibold text-slate-950 disabled:opacity-50"
          >
            {loading ? "Searching..." : "Discover Places"}
          </button>
        </div>

        {error && (
          <p className="mt-5 text-red-400">{error}</p>
        )}

        <div className="mt-10 grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {places.map((place, index) => (
            <article
              key={`${place.id}-${index}`}
              className="rounded-2xl border border-slate-700 bg-slate-900 p-5"
            >
              <h2 className="text-lg font-semibold">
                {place.name}
              </h2>

              <p className="mt-2 text-sm text-slate-400">
                {place.address || "Address unavailable"}
              </p>

              {place.rating !== null && (
                <p className="mt-3 text-amber-400">
                  Rating: {place.rating}
                </p>
              )}

              {place.maps_url && (
                <a
                  href={place.maps_url}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="mt-4 inline-block text-emerald-400"
                >
                  View on Google Maps
                </a>
              )}
            </article>
          ))}
        </div>

        {!loading && !error && places.length === 0 && (
          <p className="mt-10 text-slate-500">
            Search for a destination to discover places.
          </p>
        )}
      </div>
    </main>
  );
}