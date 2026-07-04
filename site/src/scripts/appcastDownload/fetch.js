async function appcastFetchOnce(url) {
  if (!window.__decodexAppcastPromise) {
    window.__decodexAppcastPromise = fetch(url, { mode: "cors" })
      .then(async (response) => {
        if (!response.ok) {
          throw new Error(`appcast HTTP ${response.status}`);
        }
        return response.text();
      })
      .then(appcastParse);
  }

  return window.__decodexAppcastPromise;
}
