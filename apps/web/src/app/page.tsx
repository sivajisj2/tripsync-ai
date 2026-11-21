"use client";

import {
  ArrowDown, ArrowRight, ArrowUp, ArrowUpRight, Check, Clipboard, Coffee, Compass,
  Hotel, Landmark, LoaderCircle, MapPin, Menu, Plus, Search, Share2,
  Sparkles, Star, ThumbsUp, Utensils, Users, X,
} from "lucide-react";
import { FormEvent, useCallback, useEffect, useState } from "react";
import { useTripRealtime } from "@/hooks/use-trip-realtime";

type Place = { id: string; name: string; address: string | null; rating: number | null; latitude?: number | null; longitude?: number | null; maps_url: string | null };
type Trip = { id: string; name: string; destination: string; created_at: string };
type Stop = { id: string; trip_id: string; place_id: string; name: string; address: string | null; votes: number };
type Recommendation = Place & { score: number; explanation: string[]; votes: number; category: string; in_itinerary: boolean };
type Preference = { category: string; weight: number };
type PlanItem = { stop_id: string; name: string; address: string | null; latitude: number | null; longitude: number | null; rating: number | null; category: string | null; votes: number; position: number; score: number; explanation: string };
type SmartPlan = { id: string; created_at: string; items: PlanItem[] };
type Category = { label: string; value: string; icon: typeof Landmark };

const categories: Category[] = [
  { label: "Highlights", value: "tourist attractions", icon: Landmark },
  { label: "Restaurants", value: "restaurants", icon: Utensils },
  { label: "Hotels", value: "hotels", icon: Hotel },
  { label: "Cafés", value: "cafes", icon: Coffee },
];
const cardImages = ["/travel-card-1.svg", "/travel-card-2.svg", "/travel-card-3.svg", "/travel-market.svg"];

function CategoryPill({ category, active, onSelect }: { category: Category; active: boolean; onSelect: () => void }) {
  const Icon = category.icon;
  return <button type="button" onClick={onSelect} aria-pressed={active} className={`category-pill ${active ? "category-pill-active" : ""}`}><Icon size={16} strokeWidth={1.8} /><span>{category.label}</span></button>;
}

function PlaceCard({ place, index, onAdd, adding }: { place: Place; index: number; onAdd: () => void; adding: boolean }) {
  return <article className="place-card group">
    <div className="place-image" style={{ backgroundImage: `url(${cardImages[index % cardImages.length]})` }} role="img" aria-label={`Scenic view near ${place.name}`}><div className="image-shade" /><span className="place-type">{index % 2 === 0 ? "Worth a stop" : "Local favorite"}</span></div>
    <div className="place-card-body"><div className="place-title-row"><h3>{place.name}</h3>{place.rating !== null && <span className="rating"><Star size={13} fill="currentColor" /> {place.rating.toFixed(1)}</span>}</div><p className="place-address"><MapPin size={14} /><span>{place.address || "Address unavailable"}</span></p><div className="place-actions"><button type="button" className="itinerary-link" onClick={onAdd} disabled={adding}>{adding ? <LoaderCircle className="spin" size={14} /> : <Plus size={14} />}{adding ? "Adding" : "Add to itinerary"}</button>{place.maps_url && <a href={place.maps_url} target="_blank" rel="noopener noreferrer" className="map-link">Maps <ArrowUpRight size={14} /></a>}</div></div>
  </article>;
}

function ResultsState({ loading, error, places }: { loading: boolean; error: string; places: Place[] }) {
  if (loading) return <div className="state-panel" role="status"><LoaderCircle className="spin" size={24} /><div><strong>Curating your shortlist</strong><p>Finding the places that make this destination memorable.</p></div></div>;
  if (error) return <div className="state-panel state-error" role="alert"><div className="state-icon"><X size={20} /></div><div><strong>We couldn&apos;t reach the guide</strong><p>{error}</p></div></div>;
  if (!places.length) return <div className="empty-state"><div className="empty-icon"><Compass size={22} /></div><h3>Your next good idea starts here.</h3><p>Search a city above and we&apos;ll bring back real places worth adding to your trip.</p></div>;
  return null;
}

function CreateTripDialog({ open, busy, error, onClose, onCreate }: { open: boolean; busy: boolean; error: string; onClose: () => void; onCreate: (name: string, destination: string) => void }) {
  const [name, setName] = useState("");
  const [destination, setDestination] = useState("");
  if (!open) return null;
  function submit(event: FormEvent<HTMLFormElement>) { event.preventDefault(); onCreate(name.trim(), destination.trim()); }
  return <div className="dialog-backdrop" role="presentation" onMouseDown={(event) => { if (event.currentTarget === event.target) onClose(); }}><section className="trip-dialog" role="dialog" aria-modal="true" aria-labelledby="create-trip-title"><button type="button" className="dialog-close" onClick={onClose} aria-label="Close create trip dialog"><X size={19} /></button><p className="eyebrow eyebrow-dark">A new chapter</p><h2 id="create-trip-title">Create a trip<br /><i>to look forward to.</i></h2><p className="dialog-intro">Give your shared itinerary a home. You can add real discoveries as you go.</p><form onSubmit={submit}><label htmlFor="trip-name">Trip name<input id="trip-name" value={name} onChange={(event) => setName(event.target.value)} placeholder="A long weekend in..." maxLength={100} required /></label><label htmlFor="trip-destination">Destination<input id="trip-destination" value={destination} onChange={(event) => setDestination(event.target.value)} placeholder="City, region, or country" maxLength={100} required /></label>{error && <p className="form-error" role="alert">{error}</p>}<button type="submit" className="dialog-submit" disabled={busy}>{busy ? <LoaderCircle className="spin" size={17} /> : <Plus size={17} />}{busy ? "Creating trip" : "Create trip"}</button></form></section></div>;
}

function TripWorkspace({ trips, activeTrip, stops, loading, initializing, error, sharedLinkError, onSelect, onCreate, onVote, onShare, onClearLink }: { trips: Trip[]; activeTrip: Trip | null; stops: Stop[]; loading: boolean; initializing: boolean; error: string; sharedLinkError: string; onSelect: (trip: Trip) => void; onCreate: () => void; onVote: (stop: Stop) => void; onShare: () => void; onClearLink: () => void }) {
  return <section className="trip-workspace" id="trips"><div className="workspace-top"><div><p className="eyebrow eyebrow-dark">Your travel desk</p><h2>My <i>trips.</i></h2></div><button type="button" className="forest-button" onClick={onCreate}><Plus size={16} /> Create trip</button></div>{initializing ? <div className="state-panel compact" role="status"><LoaderCircle className="spin" size={22} /><div><strong>Opening shared trip</strong><p>Fetching the latest itinerary from TripSync.</p></div></div> : sharedLinkError ? <div className="workspace-empty invalid-link" role="alert"><div className="state-icon"><X size={20} /></div><h3>This trip link isn&apos;t available.</h3><p>{sharedLinkError} The link does not provide authenticated membership.</p><button type="button" className="text-link" onClick={onClearLink}>Back to your trips <ArrowRight size={15} /></button></div> : !trips.length ? <div className="workspace-empty"><div className="empty-icon"><Compass size={22} /></div><h3>Nothing on the calendar yet.</h3><p>Create a trip, then start collecting places you want to remember.</p><button type="button" className="text-link" onClick={onCreate}>Create your first trip <ArrowRight size={15} /></button></div> : <div className="workspace-grid"><aside className="trip-list" aria-label="Your trips"><div className="trip-list-label">Saved trips <span>{trips.length}</span></div>{trips.map((trip) => <button type="button" key={trip.id} className={`trip-list-item ${activeTrip?.id === trip.id ? "trip-list-item-active" : ""}`} onClick={() => onSelect(trip)}><span>{trip.name}</span><small>{trip.destination}</small></button>)}</aside><div className="trip-detail">{activeTrip ? <><div className="trip-detail-heading"><div><span className="trip-destination"><MapPin size={14} /> {activeTrip.destination}</span><h3>{activeTrip.name}</h3></div><button type="button" className="share-button" onClick={onShare}><Share2 size={16} /> Share trip</button></div><div className="share-note"><Users size={16} /><span>Invite your travel crew with a link. Voting uses a temporary browser ID, not secure authentication.</span></div>{loading ? <div className="state-panel compact" role="status"><LoaderCircle className="spin" size={21} /><div><strong>Loading itinerary</strong><p>Bringing your saved places back.</p></div></div> : error ? <div className="state-panel state-error compact" role="alert"><div className="state-icon"><X size={18} /></div><div><strong>Couldn&apos;t load this trip</strong><p>{error}</p></div></div> : stops.length ? <div className="stops-list">{stops.map((stop) => <article className="stop-row" key={stop.id}><div className="stop-number">{String(stops.indexOf(stop) + 1).padStart(2, "0")}</div><div className="stop-copy"><h4>{stop.name}</h4><p>{stop.address || "Address unavailable"}</p></div><button type="button" className="vote-button" onClick={() => onVote(stop)} aria-label={`Vote for ${stop.name}`}><ThumbsUp size={15} /> <span>{stop.votes}</span></button></article>)}</div> : <div className="itinerary-empty"><Landmark size={23} /><h4>Your itinerary is still taking shape.</h4><p>Add places from Discover, then vote for what the group should do first.</p></div>}</> : <div className="itinerary-empty"><Compass size={23} /><h4>Select a trip to open its workspace.</h4></div>}</div></div>}</section>;
}

function SmartPlanner({ trip, stops, onAddPlace }: { trip: Trip | null; stops: Stop[]; onAddPlace: (place: Place, category: string) => void }) {
  const [recommendations, setRecommendations] = useState<Recommendation[]>([]);
  const [preferences, setPreferences] = useState<Preference[]>(categories.map(({ value }) => ({ category: value, weight: 3 })));
  const [plan, setPlan] = useState<SmartPlan | null>(null);
  const [loadedKey, setLoadedKey] = useState("");
  const [generating, setGenerating] = useState(false);
  const [savingPreferences, setSavingPreferences] = useState(false);
  const [reordering, setReordering] = useState(false);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const voteSignature = stops.map((stop) => `${stop.id}:${stop.votes}`).join("|");
  const tripId = trip?.id;
  const loadKey = tripId ? `${tripId}:${voteSignature}` : "";
  const loading = Boolean(loadKey && loadedKey !== loadKey);

  useEffect(() => {
    if (!tripId) return;
    const controller = new AbortController();
    const load = async () => {
      try {
        const [recommendationResponse, preferenceResponse, planResponse] = await Promise.all([
          fetch(`/api/trips/${encodeURIComponent(tripId)}/recommendations`, { signal: controller.signal }),
          fetch(`/api/trips/${encodeURIComponent(tripId)}/preferences`, { signal: controller.signal }),
          fetch(`/api/trips/${encodeURIComponent(tripId)}/smart-plan`, { signal: controller.signal }),
        ]);
        if (!recommendationResponse.ok) throw new Error(`Recommendations unavailable (HTTP ${recommendationResponse.status})`);
        if (!preferenceResponse.ok) throw new Error(`Preferences unavailable (HTTP ${preferenceResponse.status})`);
        if (!planResponse.ok) throw new Error(`Saved plan unavailable (HTTP ${planResponse.status})`);
        const [nextRecommendations, savedPreferences, savedPlan] = await Promise.all([
          recommendationResponse.json() as Promise<Recommendation[]>,
          preferenceResponse.json() as Promise<Preference[]>,
          planResponse.json() as Promise<SmartPlan | null>,
        ]);
        if (!controller.signal.aborted) {
          setRecommendations(nextRecommendations);
          if (savedPreferences.length) setPreferences(savedPreferences);
          setPlan(savedPlan);
          setError("");
          setLoadedKey(loadKey);
        }
      } catch (reason) {
        if (!controller.signal.aborted) {
          setError(reason instanceof Error ? reason.message : "Smart planning is temporarily unavailable.");
          setLoadedKey(loadKey);
        }
      }
    };
    void load();
    return () => controller.abort();
  }, [tripId, loadKey]);

  async function refreshRecommendations() {
    if (!trip) return;
    const response = await fetch(`/api/trips/${encodeURIComponent(trip.id)}/recommendations`);
    if (!response.ok) throw new Error(`Recommendations unavailable (HTTP ${response.status})`);
    setRecommendations(await response.json() as Recommendation[]);
  }

  async function savePreferences() {
    if (!trip) return;
    setSavingPreferences(true); setError(""); setNotice("");
    try {
      const response = await fetch(`/api/trips/${encodeURIComponent(trip.id)}/preferences`, { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ preferences }) });
      if (!response.ok) throw new Error(`Could not save group preferences (HTTP ${response.status})`);
      setPreferences(await response.json() as Preference[]);
      await refreshRecommendations();
      setNotice("Group preferences saved. Recommendations are updated.");
    } catch (reason) { setError(reason instanceof Error ? reason.message : "Could not save preferences."); }
    finally { setSavingPreferences(false); }
  }

  async function generatePlan() {
    if (!trip) return;
    setGenerating(true); setError(""); setNotice("");
    try {
      const response = await fetch(`/api/trips/${encodeURIComponent(trip.id)}/smart-plan`, { method: "POST" });
      if (!response.ok) throw new Error(`Could not generate a plan (HTTP ${response.status})`);
      setPlan((await response.json()) as SmartPlan);
      setNotice("Draft saved. Review the order before applying it to your itinerary.");
    } catch (reason) { setError(reason instanceof Error ? reason.message : "Could not generate a smart plan."); }
    finally { setGenerating(false); }
  }

  async function moveItem(index: number, offset: number) {
    if (!trip || !plan) return;
    const target = index + offset;
    if (target < 0 || target >= plan.items.length) return;
    const items = [...plan.items];
    [items[index], items[target]] = [items[target], items[index]];
    setReordering(true); setError(""); setNotice("");
    try {
      const response = await fetch(`/api/trips/${encodeURIComponent(trip.id)}/smart-plan/${encodeURIComponent(plan.id)}`, { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ stop_ids: items.map((item) => item.stop_id) }) });
      if (!response.ok) throw new Error(`Could not reorder draft (HTTP ${response.status})`);
      setPlan(await response.json() as SmartPlan);
    } catch (reason) { setError(reason instanceof Error ? reason.message : "Could not reorder draft."); }
    finally { setReordering(false); }
  }

  async function applyPlan() {
    if (!trip || !plan || !window.confirm("Apply this reviewed order to the trip itinerary? The existing stops will be reordered, not removed.")) return;
    setApplying(true); setError(""); setNotice("");
    try {
      const response = await fetch(`/api/trips/${encodeURIComponent(trip.id)}/smart-plan/${encodeURIComponent(plan.id)}/apply`, { method: "POST" });
      if (!response.ok) throw new Error(`Could not apply plan (HTTP ${response.status})`);
      setNotice("Smart plan applied to this trip.");
    } catch (reason) { setError(reason instanceof Error ? reason.message : "Could not apply plan."); }
    finally { setApplying(false); }
  }

  if (!trip) return null;
  return <section className="smart-planner" aria-labelledby="smart-plan-title">
    <div className="smart-heading"><div><p className="eyebrow eyebrow-dark">Thoughtful, not arbitrary</p><h2 id="smart-plan-title">Your group&apos;s <i>smart plan.</i></h2><p>Recommendations use live SerpApi places, trip preferences, ratings, votes, and the location details actually available.</p></div><button type="button" className="forest-button" onClick={() => void generatePlan()} disabled={generating || stops.length === 0}>{generating ? <LoaderCircle className="spin" size={16} /> : <Compass size={16} />}{generating ? "Planning" : "Generate Smart Plan"}</button></div>
    <div className="smart-preferences"><div><h3>Group preferences</h3><p>Set category interest from 0 (not for us) to 5 (love it).</p></div><div className="preference-controls">{categories.map((item) => <label key={item.value}>{item.label}<select aria-label={`${item.label} group preference`} value={preferences.find((preference) => preference.category === item.value)?.weight ?? 3} onChange={(event) => setPreferences((current) => [...current.filter((preference) => preference.category !== item.value), { category: item.value, weight: Number(event.target.value) }])}>{[0, 1, 2, 3, 4, 5].map((weight) => <option key={weight} value={weight}>{weight}</option>)}</select></label>)}</div><button type="button" className="outline-button" onClick={() => void savePreferences()} disabled={savingPreferences}>{savingPreferences ? <LoaderCircle className="spin" size={15} /> : <Check size={15} />}{savingPreferences ? "Saving" : "Save preferences"}</button></div>
    {error && <div className="state-panel state-error compact" role="alert"><div className="state-icon"><X size={18} /></div><div><strong>Smart planning needs attention</strong><p>{error}</p></div></div>}{notice && <p className="smart-notice" role="status"><Check size={15} />{notice}</p>}
    <div className="smart-columns"><div className="recommendation-column"><div className="smart-subheading"><div><span className="eyebrow eyebrow-dark">Live discoveries</span><h3>Worth considering</h3></div>{loading && <LoaderCircle className="spin" size={18} />}</div>{loading ? <div className="smart-empty" role="status">Ranking real places for your group…</div> : recommendations.length === 0 ? <div className="smart-empty">No live places are available for this destination right now.</div> : <div className="recommendation-list">{recommendations.slice(0, 8).map((item, index) => <article className="recommendation-card" key={`${item.id}-${item.category}`}><div className="recommendation-art" style={{ backgroundImage: `url(${cardImages[index % cardImages.length]})` }} aria-hidden="true"/><div className="recommendation-copy"><div className="recommendation-title"><div><h4>{item.name}</h4><span>{item.category}{item.rating !== null ? ` · ${item.rating.toFixed(1)} rating` : " · rating unavailable"}</span></div><strong>{item.score.toFixed(0)}<small>/100</small></strong></div><ul>{item.explanation.slice(0, 3).map((reason) => <li key={reason}>{reason}</li>)}</ul><button type="button" className="itinerary-link" disabled={item.in_itinerary} onClick={() => onAddPlace(item, item.category)}>{item.in_itinerary ? <Check size={14} /> : <Plus size={14} />}{item.in_itinerary ? "Already in itinerary" : "Add to itinerary"}</button></div></article>)}</div>}</div>
      <div className="plan-column"><div className="smart-subheading"><div><span className="eyebrow eyebrow-dark">Draft itinerary</span><h3>Review the route</h3></div>{plan && <span className="result-count">{plan.items.length} stops</span>}</div>{!plan ? <div className="smart-empty"><div className="empty-icon"><Compass size={20} /></div><h4>{stops.length ? "Ready when you are." : "Add places to start planning."}</h4><p>The generator considers votes, ratings, group preferences, and uses nearest-neighbor ordering where coordinates exist.</p></div> : plan.items.length === 0 ? <div className="smart-empty">There are no saved stops to arrange yet. Add a place, then generate a plan.</div> : <><ol className="plan-list">{plan.items.map((item, index) => <li className="plan-item" key={item.stop_id}><span className="plan-index">{String(index + 1).padStart(2, "0")}</span><div className="plan-item-copy"><h4>{item.name}</h4><p>{item.explanation}</p></div><div className="plan-reorder"><button type="button" aria-label={`Move ${item.name} up`} disabled={reordering || index === 0} onClick={() => void moveItem(index, -1)}><ArrowUp size={15} /></button><button type="button" aria-label={`Move ${item.name} down`} disabled={reordering || index === plan.items.length - 1} onClick={() => void moveItem(index, 1)}><ArrowDown size={15} /></button></div></li>)}</ol><button type="button" className="apply-plan-button" disabled={applying || reordering} onClick={() => void applyPlan()}>{applying ? <LoaderCircle className="spin" size={16} /> : <Check size={16} />}{applying ? "Applying" : "Apply reviewed order"}</button></>}</div></div>
    <p className="scoring-note">Score weights: rating 30%, category preference 30%, group votes 25%, coordinate availability 15%. Missing rating/location values use neutral scoring and are labeled; no distance or travel-time estimates are invented.</p>
  </section>;
}

export default function Home() {
  const [city, setCity] = useState("");
  const [category, setCategory] = useState("tourist attractions");
  const [places, setPlaces] = useState<Place[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [menuOpen, setMenuOpen] = useState(false);
  const [trips, setTrips] = useState<Trip[]>([]);
  const [activeTrip, setActiveTrip] = useState<Trip | null>(null);
  const [stops, setStops] = useState<Stop[]>([]);
  const [stopsLoading, setStopsLoading] = useState(false);
  const [tripError, setTripError] = useState("");
  const [sharedLinkError, setSharedLinkError] = useState("");
  const [tripInitializing, setTripInitializing] = useState(false);
  const [voterId] = useState(() => { if (typeof window === "undefined") return ""; const stored = localStorage.getItem("tripsync-demo-voter"); const id = stored || (crypto.randomUUID ? crypto.randomUUID() : `${Date.now()}-${Math.random()}`); if (!stored) localStorage.setItem("tripsync-demo-voter", id); return id; });
  const [dialogOpen, setDialogOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState("");
  const [addingPlace, setAddingPlace] = useState("");
  const [shareCopied, setShareCopied] = useState(false);

  async function request<T>(path: string, options?: RequestInit): Promise<T> { const response = await fetch(`/api${path}`, { headers: { "Content-Type": "application/json", ...(options?.headers || {}) }, ...options }); if (!response.ok) { const message = await response.text(); throw new Error(message || `Request failed (HTTP ${response.status})`); } return response.status === 204 ? (undefined as T) : response.json() as Promise<T>; }
  const loadStops = useCallback(async (trip: Trip) => { setStopsLoading(true); setTripError(""); try { setStops(await request<Stop[]>(`/trips/${trip.id}/stops`)); } catch (err) { setTripError(err instanceof Error ? err.message : "Could not load itinerary"); } finally { setStopsLoading(false); } }, []);
  async function selectTrip(trip: Trip) { setActiveTrip(trip); window.history.replaceState(null, "", `?trip=${encodeURIComponent(trip.id)}#trips`); await loadStops(trip); }
  async function loadSavedTrips() { const queryTrip = new URLSearchParams(window.location.search).get("trip"); setTripInitializing(Boolean(queryTrip)); setSharedLinkError(""); try { let sharedTrip: Trip | null = null; if (queryTrip) { try { sharedTrip = await request<Trip>(`/trips/${encodeURIComponent(queryTrip)}`); } catch { setSharedLinkError("This trip may have been deleted or the URL may be invalid."); } } const ids = JSON.parse(localStorage.getItem("tripsync-trip-ids") || "[]") as string[]; const saved = await Promise.all(ids.filter((id) => id !== queryTrip).map(async (id) => { try { return await request<Trip>(`/trips/${encodeURIComponent(id)}`); } catch { return null; } })); const valid = saved.filter((trip): trip is Trip => trip !== null); const combined = sharedTrip ? [sharedTrip, ...valid] : valid; setTrips(combined); const selected = sharedTrip || (!queryTrip ? valid[0] : null); if (selected) { setActiveTrip(selected); await loadStops(selected); } else if (queryTrip) { setActiveTrip(null); setStops([]); } } finally { setTripInitializing(false); } }
  // Load persisted trip IDs once the browser is available.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { const timer = window.setTimeout(() => { void loadSavedTrips(); }, 0); return () => window.clearTimeout(timer); }, []);

  async function searchPlaces(event?: FormEvent<HTMLFormElement>) { event?.preventDefault(); const destination = city.trim(); if (!destination) return; setLoading(true); setError(""); try { const params = new URLSearchParams({ city: destination, category }); setPlaces(await request<Place[]>(`/places/search?${params.toString()}`)); } catch (err) { setPlaces([]); setError(err instanceof Error ? err.message : "Unknown error"); } finally { setLoading(false); } }
  async function createTrip(name: string, destination: string) { if (!name || !destination) return; setCreating(true); setCreateError(""); try { const trip = await request<Trip>("/trips", { method: "POST", body: JSON.stringify({ name, destination }) }); const ids = JSON.parse(localStorage.getItem("tripsync-trip-ids") || "[]") as string[]; localStorage.setItem("tripsync-trip-ids", JSON.stringify([...new Set([trip.id, ...ids])])); setTrips((current) => [trip, ...current.filter((item) => item.id !== trip.id)]); setDialogOpen(false); await selectTrip(trip); document.getElementById("trips")?.scrollIntoView({ behavior: "smooth" }); } catch (err) { setCreateError(err instanceof Error ? err.message : "Could not create trip"); } finally { setCreating(false); } }
  async function addToItinerary(place: Place, placeCategory = category) { if (!activeTrip) { setDialogOpen(true); return; } setAddingPlace(place.id); setTripError(""); try { await request(`/trips/${activeTrip.id}/stops`, { method: "POST", body: JSON.stringify({ place_id: place.id, name: place.name, address: place.address, latitude: place.latitude, longitude: place.longitude, rating: place.rating, category: placeCategory }) }); await loadStops(activeTrip); document.getElementById("trips")?.scrollIntoView({ behavior: "smooth" }); } catch (err) { setTripError(err instanceof Error ? err.message : "Could not add this place"); } finally { setAddingPlace(""); } }
  async function vote(stop: Stop) { if (!activeTrip || !voterId) return; setTripError(""); try { await request(`/trips/${activeTrip.id}/votes`, { method: "POST", body: JSON.stringify({ stop_id: stop.id, voter_id: voterId }) }); await loadStops(activeTrip); } catch (err) { const message = err instanceof Error ? err.message : "Could not record vote"; setTripError(message.includes("Already voted") ? "You already voted for this place from this browser." : message); } }
  async function shareTrip() { if (!activeTrip) return; const url = `${window.location.origin}/?trip=${encodeURIComponent(activeTrip.id)}#trips`; await navigator.clipboard?.writeText(url); setShareCopied(true); window.setTimeout(() => setShareCopied(false), 2200); }
  function clearSharedLink() { window.history.replaceState(null, "", "/#trips"); setSharedLinkError(""); setActiveTrip(trips[0] || null); if (trips[0]) void loadStops(trips[0]); }
  const refreshTripStops = useCallback(() => { if (activeTrip) void loadStops(activeTrip); }, [activeTrip, loadStops]);
  useTripRealtime(activeTrip?.id || null, refreshTripStops);

  return <main className="site-shell"><nav className="site-nav" aria-label="Main navigation"><a href="#top" className="brand" aria-label="TripSync AI home"><span className="brand-mark"><Compass size={19} strokeWidth={2.1} /></span><span>TripSync <em>AI</em></span></a><div className={`nav-links ${menuOpen ? "nav-links-open" : ""}`}><a className="nav-link nav-link-active" href="#discover" onClick={() => setMenuOpen(false)}>Discover</a><a className="nav-link" href="#trips" onClick={() => setMenuOpen(false)}>My trips</a><a className="nav-link" href="#how-it-works" onClick={() => setMenuOpen(false)}>How it works</a><button type="button" className="nav-close" aria-label="Close menu" onClick={() => setMenuOpen(false)}><X size={20} /></button></div><div className="nav-actions"><span className="nav-sign-in">Sign in</span><button type="button" className="nav-join" onClick={() => setDialogOpen(true)}>Join free <ArrowRight size={15} /></button><button type="button" className="menu-button" aria-label="Open menu" aria-expanded={menuOpen} onClick={() => setMenuOpen(true)}><Menu size={21} /></button></div></nav>
    <section className="hero" id="top"><div className="hero-copy"><p className="eyebrow"><Sparkles size={14} /> The considered way to travel</p><h1>Go somewhere<br /><span>worth remembering.</span></h1><p className="hero-description">Discover the places, rituals, and little detours that turn a trip into a story worth telling.</p></div><div className="hero-photo" role="img" aria-label="Aerial view of a tropical coast"><div className="hero-photo-caption"><span>01 / 04</span><span>Somewhere beautiful</span></div></div></section>
    <section className="discover-section" id="discover"><div className="section-heading"><div><p className="eyebrow eyebrow-dark">Start with a feeling</p><h2>Find your next <i>favorite</i> place.</h2></div><p className="section-note">Real recommendations from Google Maps,<br className="desktop-only" /> shaped around where you want to go.</p></div><form className="search-bar" onSubmit={searchPlaces}><div className="search-field"><MapPin size={19} /><label htmlFor="destination">Where to?</label><input id="destination" value={city} onChange={(event) => setCity(event.target.value)} placeholder="City, region, or country" autoComplete="off" /></div><button type="submit" className="search-submit" disabled={loading || !city.trim()}>{loading ? <LoaderCircle className="spin" size={18} /> : <Search size={18} />}<span>{loading ? "Searching" : "Discover"}</span></button></form><div className="category-row" aria-label="Place categories">{categories.map((item) => <CategoryPill key={item.value} category={item} active={category === item.value} onSelect={() => setCategory(item.value)} />)}</div><ResultsState loading={loading} error={error} places={places} />{places.length > 0 && !loading && <div className="results-section"><div className="results-heading"><div><p className="eyebrow eyebrow-dark">Curated for you</p><h2>Places to linger in <i>{city.trim()}</i></h2></div><span className="result-count">{places.length} discoveries</span></div><div className="places-grid">{places.map((place, index) => <PlaceCard key={`${place.id}-${index}`} place={place} index={index} adding={addingPlace === place.id} onAdd={() => void addToItinerary(place)} />)}</div></div>}</section>
    <TripWorkspace trips={trips} activeTrip={activeTrip} stops={stops} loading={stopsLoading} initializing={tripInitializing} error={tripError} sharedLinkError={sharedLinkError} onSelect={(trip) => void selectTrip(trip)} onCreate={() => { setCreateError(""); setDialogOpen(true); }} onVote={(stop) => void vote(stop)} onShare={() => void shareTrip()} onClearLink={clearSharedLink} />
    <SmartPlanner trip={activeTrip} stops={stops} onAddPlace={(place, placeCategory) => void addToItinerary(place, placeCategory)} />
    <section className="editorial-strip" id="how-it-works"><div className="strip-image" role="img" aria-label="Travelers walking through a market" /><div className="strip-copy"><p className="eyebrow eyebrow-dark">Travel, in good company</p><h2>Better together,<br /><i>by design.</i></h2><p>Keep the group chat for catching up. TripSync gives everyone a shared place to discover, discuss, and decide.</p><a href="#discover" className="text-link">Start planning together <ArrowUpRight size={15} /></a></div><div className="strip-stat"><Users size={19} /><span>Made for the<br /><strong>whole crew</strong></span></div></section><footer className="site-footer"><a href="#top" className="brand"><span className="brand-mark"><Compass size={18} strokeWidth={2.1} /></span><span>TripSync <em>AI</em></span></a><p>Thoughtful travel, made simple.</p><span className="footer-meta">© 2025 TripSync AI</span></footer><CreateTripDialog open={dialogOpen} busy={creating} error={createError} onClose={() => setDialogOpen(false)} onCreate={(name, destination) => void createTrip(name, destination)} />{shareCopied && <div className="toast" role="status"><Check size={16} /> Share link copied <Clipboard size={14} /></div>}</main>;
}
