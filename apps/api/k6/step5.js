import http from "k6/http";
import { check, sleep } from "k6";

const baseUrl = __ENV.BASE_URL || "http://localhost:8080";
const tripId = __ENV.TRIP_ID;
const stopId = __ENV.STOP_ID;

if (!tripId || !stopId) {
  throw new Error("Set TRIP_ID and STOP_ID to existing database records before running this test.");
}

export const options = {
  scenarios: {
    trip_reads: { executor: "constant-vus", vus: 5, duration: "30s", exec: "readTrip" },
    recommendation_reads: { executor: "constant-vus", vus: 2, duration: "30s", exec: "recommend" },
    voting_writes: { executor: "constant-vus", vus: 5, duration: "30s", exec: "vote" },
  },
};

export function readTrip() {
  const trip = http.get(`${baseUrl}/api/trips/${tripId}`);
  const stops = http.get(`${baseUrl}/api/trips/${tripId}/stops`);
  check(trip, { "trip read returned 200": (response) => response.status === 200 });
  check(stops, { "stop list returned 200": (response) => response.status === 200 });
  sleep(0.2);
}

export function recommend() {
  const response = http.get(`${baseUrl}/api/trips/${tripId}/recommendations`);
  check(response, { "recommendations returned 200": (result) => result.status === 200 });
  sleep(1);
}

export function vote() {
  const suffix = `${__VU.toString(16).padStart(4, "0")}${__ITER.toString(16).padStart(8, "0")}`;
  const voterId = `00000000-0000-4000-8000-${suffix}`;
  const response = http.post(
    `${baseUrl}/api/trips/${tripId}/votes`,
    JSON.stringify({ stop_id: stopId, voter_id: voterId }),
    { headers: { "Content-Type": "application/json" } },
  );
  check(response, { "vote was accepted": (result) => result.status === 201 });
  sleep(0.2);
}
