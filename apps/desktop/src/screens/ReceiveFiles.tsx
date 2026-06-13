import { Radio, Server } from "lucide-react";
import type { ReceiverEndpoint } from "../api/desktop";

type ReceiveFilesProps = {
  receiver: ReceiverEndpoint | null;
  receiveDir: string | null;
  onReceive: () => void;
};

export function ReceiveFiles({
  receiver,
  receiveDir,
  onReceive,
}: ReceiveFilesProps) {
  return (
    <section className="screen">
      <div className="screenHeader">
        <div>
          <h1>Receive Files</h1>
        </div>
        <button onClick={onReceive} title="Start receiver">
          <Radio size={18} />
          Listen
        </button>
      </div>

      <div className="panel">
        <div className="panelHeader">
          <Server size={19} />
          <h2>Receiver Endpoint</h2>
        </div>
        <code className="pathLine">{receiver?.address ?? "Not listening"}</code>
      </div>

      <div className="panel">
        <div className="panelHeader">
          <Radio size={19} />
          <h2>Destination</h2>
        </div>
        <code className="pathLine">{receiveDir ?? "Loading"}</code>
      </div>
    </section>
  );
}
