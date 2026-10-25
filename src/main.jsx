import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App.jsx';
import StartupFailure from './components/StartupFailure.jsx';
import NoCompanyOpen from './components/CompanyFiles.jsx';
import SignIn from './components/Users.jsx';
import { api } from './lib/api.js';
import './index.css';
import './styles/print.css';

const root = ReactDOM.createRoot(document.getElementById('root'));

// If the company file could not be opened at launch, explain that instead of starting the app; with
// no company open (File › Close Company), show the No Company Open screen; with a company that has
// users, ask who is signing in (and have a temporary password replaced) first.
async function firstScreen() {
  const failure = await api.getStartupFailure().catch(() => null);
  if (failure) return <StartupFailure failure={failure} />;
  const status = await api.getStorageStatus().catch(() => undefined);
  if (status === null) return <NoCompanyOpen />;
  const signIn = await api.getSignInState().catch(() => null);
  // "Remember me on this Mac": once per opening of the file, never after a sign-out or idle lock.
  if (signIn?.usersEnabled && !signIn.user && (await api.signInRemembered().catch(() => null))) return <App />;
  if (signIn?.usersEnabled && (!signIn.user || signIn.user.mustChangePassword)) {
    return <SignIn companyName={status?.companyName} user={signIn.user} />;
  }
  return <App />;
}

firstScreen().then((screen) => root.render(<React.StrictMode>{screen}</React.StrictMode>));
