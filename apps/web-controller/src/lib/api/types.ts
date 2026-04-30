export interface ApiErrorResponse {
  error: string;
  message?: string;
}

export interface SessionRef {
  id: string;
  hostId: string;
  createdAt: string;
}
