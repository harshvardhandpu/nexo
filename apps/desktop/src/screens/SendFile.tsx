import { Send, Server } from "lucide-react";
import type { ReceiverEndpoint } from "../api/desktop";

type SendFileProps = {
  receiver: ReceiverEndpoint | null;
  filePath: string;
  host: string;
  error: string | null;
  onFilePathChange: (value: string) => void;
  onHostChange: (value: string) => void;
  onSend: () => void;
};

export function SendFile({
  receiver,
  filePath,
  host,
  error,
  onFilePathChange,
  onHostChange,
  onSend,
}: SendFileProps) {
  return (
    <section className="screen">
      <div className="screenHeader">
        <div>
          <h1>Send File</h1>
        </div>
        <button onClick={onSend} title="Start send">
          <Send size={18} />
          Send
        </button>
      </div>

      {error ? <div className="errorBanner">{error}</div> : null}

      <div className="formGrid">
        <label>
          <span>File path</span>
          <input
            value={filePath}
            onChange={(event) => onFilePathChange(event.target.value)}
            placeholder="/home/user/file.bin"
          />
        </label>
        <label>
          <span>Receiver address</span>
          <input
            value={host}
            onChange={(event) => onHostChange(event.target.value)}
            placeholder={receiver?.address ?? "127.0.0.1:port"}
          />
        </label>
      </div>

      <div className="panel">
        <div className="panelHeader">
          <Server size={19} />
          <h2>Stored Receiver</h2>
        </div>
        <code className="pathLine">{receiver?.address ?? "No receiver advertised"}</code>
      </div>
    </section>
  );
}
