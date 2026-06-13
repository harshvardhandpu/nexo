import { RefreshCcw, Wifi } from "lucide-react";
import type { Peer } from "../api/desktop";

type DiscoverPeersProps = {
  peers: Peer[];
  loading: boolean;
  error: string | null;
  onRefresh: () => void;
};

export function DiscoverPeers({
  peers,
  loading,
  error,
  onRefresh,
}: DiscoverPeersProps) {
  return (
    <section className="screen">
      <div className="screenHeader">
        <div>
          <h1>Discover Peers</h1>
        </div>
        <button onClick={onRefresh} disabled={loading} title="Refresh peers">
          <RefreshCcw className={loading ? "spin" : ""} size={18} />
          Refresh
        </button>
      </div>

      {error ? <div className="errorBanner">{error}</div> : null}

      <div className="table">
        <div className="table__row table__row--head">
          <span>Peer</span>
          <span>Addresses</span>
          <span>Port</span>
        </div>
        {peers.length === 0 ? (
          <div className="empty">
            <Wifi size={22} />
            No peers found.
          </div>
        ) : (
          peers.map((peer) => (
            <div className="table__row" key={peer.peerId}>
              <div>
                <strong>{peer.displayName}</strong>
                <small>{peer.peerId}</small>
              </div>
              <span>{peer.addresses.join(", ")}</span>
              <span>{peer.port}</span>
            </div>
          ))
        )}
      </div>
    </section>
  );
}
