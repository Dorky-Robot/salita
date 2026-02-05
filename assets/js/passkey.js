// Base64url encoding/decoding helpers for WebAuthn
function base64urlToBuffer(base64url) {
  const base64 = base64url.replace(/-/g, '+').replace(/_/g, '/');
  const pad = base64.length % 4 === 0 ? '' : '='.repeat(4 - (base64.length % 4));
  const binary = atob(base64 + pad);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes.buffer;
}

function bufferToBase64url(buffer) {
  const bytes = new Uint8Array(buffer);
  let binary = '';
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

// Recursively convert base64url strings in a credential creation options object to ArrayBuffers
function decodeCreationOptions(options) {
  options.challenge = base64urlToBuffer(options.challenge);
  options.user.id = base64urlToBuffer(options.user.id);
  if (options.excludeCredentials) {
    options.excludeCredentials = options.excludeCredentials.map(function(cred) {
      cred.id = base64urlToBuffer(cred.id);
      return cred;
    });
  }
  return options;
}

// Recursively convert base64url strings in a credential request options object to ArrayBuffers
function decodeRequestOptions(options) {
  options.challenge = base64urlToBuffer(options.challenge);
  if (options.allowCredentials) {
    options.allowCredentials = options.allowCredentials.map(function(cred) {
      cred.id = base64urlToBuffer(cred.id);
      return cred;
    });
  }
  return options;
}

// Register a new passkey (used on /auth/setup page)
async function registerPasskey() {
  const usernameInput = document.getElementById('username');
  const displayNameInput = document.getElementById('display_name');
  const statusEl = document.getElementById('status');

  const username = usernameInput.value.trim();
  if (!username) {
    statusEl.textContent = 'Please enter a username.';
    statusEl.className = 'text-red-600 text-sm mt-2';
    return;
  }

  const displayName = displayNameInput ? displayNameInput.value.trim() : username;

  statusEl.textContent = 'Starting registration...';
  statusEl.className = 'text-stone-600 text-sm mt-2';

  try {
    // Step 1: Get challenge from server
    const startResp = await fetch('/auth/setup/start', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ username: username, display_name: displayName }),
    });

    if (!startResp.ok) {
      const err = await startResp.text();
      throw new Error(err);
    }

    const startData = await startResp.json();
    const creationOptions = decodeCreationOptions(startData.publicKey);

    // Step 2: Create credential via browser
    statusEl.textContent = 'Touch your authenticator...';
    const credential = await navigator.credentials.create({ publicKey: creationOptions });

    // Step 3: Send credential to server
    const attestationResponse = credential.response;
    const finishBody = {
      id: credential.id,
      rawId: bufferToBase64url(credential.rawId),
      type: credential.type,
      response: {
        attestationObject: bufferToBase64url(attestationResponse.attestationObject),
        clientDataJSON: bufferToBase64url(attestationResponse.clientDataJSON),
      },
    };

    // Include extensions if present
    const extensions = credential.getClientExtensionResults();
    if (Object.keys(extensions).length > 0) {
      finishBody.extensions = extensions;
    }

    const finishResp = await fetch('/auth/setup/finish', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(finishBody),
    });

    if (!finishResp.ok) {
      const err = await finishResp.text();
      throw new Error(err);
    }

    statusEl.textContent = 'Registration successful! Redirecting...';
    statusEl.className = 'text-green-600 text-sm mt-2';
    window.location.href = '/';
  } catch (err) {
    console.error('Registration error:', err);
    statusEl.textContent = 'Registration failed: ' + err.message;
    statusEl.className = 'text-red-600 text-sm mt-2';
  }
}

// Authenticate with an existing passkey (used on /auth/login page)
async function authenticatePasskey() {
  const statusEl = document.getElementById('status');

  statusEl.textContent = 'Starting authentication...';
  statusEl.className = 'text-stone-600 text-sm mt-2';

  try {
    // Step 1: Get challenge from server
    const startResp = await fetch('/auth/login/start', {
      method: 'POST',
    });

    if (!startResp.ok) {
      const err = await startResp.text();
      throw new Error(err);
    }

    const startData = await startResp.json();
    const requestOptions = decodeRequestOptions(startData.publicKey);

    // Step 2: Get assertion via browser
    statusEl.textContent = 'Touch your authenticator...';
    const assertion = await navigator.credentials.get({ publicKey: requestOptions });

    // Step 3: Send assertion to server
    const assertionResponse = assertion.response;
    const finishBody = {
      id: assertion.id,
      rawId: bufferToBase64url(assertion.rawId),
      type: assertion.type,
      response: {
        authenticatorData: bufferToBase64url(assertionResponse.authenticatorData),
        clientDataJSON: bufferToBase64url(assertionResponse.clientDataJSON),
        signature: bufferToBase64url(assertionResponse.signature),
      },
    };

    if (assertionResponse.userHandle) {
      finishBody.response.userHandle = bufferToBase64url(assertionResponse.userHandle);
    }

    // Include extensions if present
    const extensions = assertion.getClientExtensionResults();
    if (Object.keys(extensions).length > 0) {
      finishBody.extensions = extensions;
    }

    const finishResp = await fetch('/auth/login/finish', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(finishBody),
    });

    if (!finishResp.ok) {
      const err = await finishResp.text();
      throw new Error(err);
    }

    statusEl.textContent = 'Login successful! Redirecting...';
    statusEl.className = 'text-green-600 text-sm mt-2';
    window.location.href = '/';
  } catch (err) {
    console.error('Authentication error:', err);
    statusEl.textContent = 'Login failed: ' + err.message;
    statusEl.className = 'text-red-600 text-sm mt-2';
  }
}
