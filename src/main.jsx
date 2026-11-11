import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App.jsx';
import StartupFailure from './components/StartupFailure.jsx';
import NoCompanyOpen from './components/CompanyFiles.jsx';
import SignIn from './components/Users.jsx';
import { OpeningOverlay, OpeningScreen } from './components/Opening.jsx';
import { api } from './lib/api.js';
import { loadStartPage } from './lib/windows.js';
import { installMyPreferenceBehaviours } from './lib/myPreferences.js';
import { installCalculatorTape } from './lib/calcTape.js';
import './index.css';
import './styles/print.css';

const root = ReactDOM.createRoot(document.getElementById('root'));

// Preferences › General: Enter moves between fields, tooltips for clipped text.
installMyPreferenceBehaviours();
// Arithmetic in amount and quantity boxes, with a tape of the running total.
installCalculatorTape();

// While the company file opens at launch, say which and how far it is. If it could not be opened, explain that instead of starting the app; with
// no company open (File › Close Company), show the No Company Open screen; with a company that has
// users, ask who is signing in (and have a temporary password replaced) first. The app itself starts
// once it knows its page: the saved desktop's, when the company has just opened.
async function app() {
  await loadStartPage();
  return <App />;
}

async function firstScreen() {
  // The company file is still opening (gap R28): say so, then start as usual once it is open.
  const opening = await api.getOpeningState().catch(() => null);
  if (opening) return <OpeningScreen initial={opening} onOpened={start} />;
  const failure = await api.getStartupFailure().catch(() => null);
  if (failure) return <StartupFailure failure={failure} />;
  const status = await api.getStorageStatus().catch(() => undefined);
  if (status === null) return <NoCompanyOpen />;
  const signIn = await api.getSignInState().catch(() => null);
  // "Remember me on this computer": once per opening of the file, never after a sign-out or idle lock.
  if (signIn?.usersEnabled && !signIn.user && (await api.signInRemembered().catch(() => null))) return app();
  if (signIn?.usersEnabled && (!signIn.user || signIn.user.mustChangePassword || signIn.user.mustSetUpTwoStep)) {
    return <SignIn companyName={status?.companyName} user={signIn.user} encrypted={signIn.encrypted} />;
  }
  return app();
}

// Opens from inside the app show Opening <name>… over whatever screen is up.
function start() {
  firstScreen().then((screen) =>
    root.render(
      <React.StrictMode>
        {screen}
        {screen.type !== OpeningScreen && <OpeningOverlay />}
      </React.StrictMode>,
    ),
  );
}

start();
