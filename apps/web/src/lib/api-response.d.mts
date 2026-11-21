export function parseApiResponse<T>(response: Response): Promise<T>;
export function apiRequest<T>(path: string, options?: RequestInit): Promise<T>;
