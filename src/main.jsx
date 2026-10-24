import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App.jsx';
import StartupFailure from './components/StartupFailure.jsx';
import NoCompanyOpen from './components/CompanyFiles.jsx';
import { api } from './lib/api.js';
import './index.css';
import './styles/print.css';

const root = ReactDOM.createRoot(document.getElementById('root'));

// If the company file could not be opened at launch, explain that instead of starting the app; with
// no company open (File › Close Company), show the No Company Open screen.
async function firstScreen() {
  const failure = await api.getStartupFailure().catch(() => null);
  if (failure) return <StartupFailure failure={failure} />;
  const status = await api.getStorageStatus().catch(() => undefined);
  return status === null ? <NoCompanyOpen /> : <App />;
}

firstScreen().then((screen) => root.render(<React.StrictMode>{screen}</React.StrictMode>));
