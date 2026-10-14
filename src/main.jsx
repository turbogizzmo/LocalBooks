import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App.jsx';
import StartupFailure from './components/StartupFailure.jsx';
import { api } from './lib/api.js';
import './index.css';
import './styles/print.css';

const root = ReactDOM.createRoot(document.getElementById('root'));

// If the company file could not be opened at launch, explain that instead of starting the app.
api
  .getStartupFailure()
  .catch(() => null)
  .then((failure) =>
    root.render(<React.StrictMode>{failure ? <StartupFailure failure={failure} /> : <App />}</React.StrictMode>),
  );
