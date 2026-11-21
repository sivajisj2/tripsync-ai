import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "standalone",
  cacheComponents: true,
  partialPrefetching: true,
  async rewrites() {
    const apiOrigin = process.env.API_INTERNAL_URL || "http://localhost:8080";
    return [{ source: "/api/:path*", destination: `${apiOrigin}/api/:path*` }];
  },
};

export default nextConfig;
