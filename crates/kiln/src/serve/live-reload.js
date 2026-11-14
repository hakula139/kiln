(function () {
  let ws = null;
  let retry = null;
  let active = true;

  const connect = () => {
    if (!active || ws) {
      return;
    }

    const url = `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/__kiln_live_reload`;
    const socket = new WebSocket(url);
    ws = socket;

    socket.onmessage = (e) => {
      if (e.data === 'reload') {
        window.location.reload();
      }
    };

    socket.onclose = () => {
      if (ws !== socket) {
        return;
      }

      ws = null;

      if (active) {
        retry = setTimeout(() => {
          retry = null;
          connect();
        }, 1000);
      }
    };
  };

  window.addEventListener('pagehide', () => {
    active = false;
    clearTimeout(retry);
    retry = null;

    const socket = ws;
    ws = null;
    if (socket) {
      socket.close();
    }
  });

  window.addEventListener('pageshow', () => {
    active = true;
    connect();
  });

  connect();
})();
