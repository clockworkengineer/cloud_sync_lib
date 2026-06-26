import http.server
import socketserver
import webbrowser
import urllib.parse
import urllib.request
import json
import re
import sys
from pathlib import Path

PORT = 8080
REDIRECT_URI = f"http://localhost:{PORT}"
CONFIG_PATH = Path(sys.argv[1] if len(sys.argv) > 1 else "private_config.toml")

def read_config():
    if not CONFIG_PATH.exists():
        print(f"Error: {CONFIG_PATH} not found.")
        sys.exit(1)
    
    content = CONFIG_PATH.read_text()
    
    # We want to extract client_id and client_secret under [onedrive_credentials]
    # Let's parse the section to avoid matching Google or Dropbox credentials.
    sections = re.split(r'\n\s*\[([^\]]+)\]', content)
    
    client_id = None
    client_secret = None
    
    # sections[0] is everything before the first [section]
    # Then we have pairs of (section_name, section_content)
    current_section = None
    for i in range(1, len(sections), 2):
        sec_name = sections[i].strip()
        sec_content = sections[i+1]
        if sec_name == "onedrive_credentials":
            client_id_match = re.search(r'client_id\s*=\s*"([^"]+)"', sec_content)
            client_secret_match = re.search(r'client_secret\s*=\s*"([^"]+)"', sec_content)
            if client_id_match:
                client_id = client_id_match.group(1)
            if client_secret_match:
                client_secret = client_secret_match.group(1)
            break
            
    if not client_id or not client_secret:
        print("Error: Could not find client_id or client_secret under [onedrive_credentials] in private_config.toml.")
        sys.exit(1)
        
    return client_id, client_secret

def update_config(refresh_token):
    content = CONFIG_PATH.read_text()
    
    # We want to replace refresh_token specifically within the [onedrive_credentials] section
    sections = re.split(r'(\n\s*\[[^\]]+\])', content)
    
    # Find the [onedrive_credentials] section and update its content
    updated = False
    for i in range(len(sections)):
        if "onedrive_credentials" in sections[i]:
            # The next element sections[i+1] contains the values for this section
            if i + 1 < len(sections):
                sec_content = sections[i+1]
                # Replace refresh_token value
                new_sec_content = re.sub(
                    r'(refresh_token\s*=\s*")[^"]*(")',
                    rf'\g<1>{refresh_token}\g<2>',
                    sec_content
                )
                # Also set enabled = true
                new_sec_content = re.sub(
                    r'(enabled\s*=\s*)false',
                    r'\g<1>true',
                    new_sec_content
                )
                sections[i+1] = new_sec_content
                updated = True
                break
                
    if updated:
        CONFIG_PATH.write_text("".join(sections))
        print(f"\n[+] Successfully updated {CONFIG_PATH} with the new OneDrive refresh_token and enabled OneDrive sync!")
    else:
        print("\n[-] Error: Failed to find and update [onedrive_credentials] in private_config.toml")

def exchange_code_for_token(code, client_id, client_secret):
    url = "https://login.microsoftonline.com/common/oauth2/v2.0/token"
    
    data = urllib.parse.urlencode({
        "client_id": client_id,
        "client_secret": client_secret,
        "code": code,
        "redirect_uri": REDIRECT_URI,
        "grant_type": "authorization_code"
    }).encode("utf-8")
    
    req = urllib.request.Request(url, data=data)
    req.add_header("Content-Type", "application/x-www-form-urlencoded")
    
    try:
        with urllib.request.urlopen(req) as response:
            res_data = json.loads(response.read().decode("utf-8"))
            return res_data.get("refresh_token")
    except Exception as e:
        print(f"Error exchanging code for token: {e}")
        if hasattr(e, 'read'):
            print(e.read().decode("utf-8"))
        sys.exit(1)

class OAuthHandler(http.server.SimpleHTTPRequestHandler):
    auth_code = None

    def do_GET(self):
        query = urllib.parse.urlparse(self.path).query
        params = urllib.parse.parse_qs(query)
        
        if "code" in params:
            OAuthHandler.auth_code = params["code"][0]
            self.send_response(200)
            self.send_header("Content-type", "text/html")
            self.end_headers()
            self.wfile.write(b"<html><body><h1>Authorization Successful!</h1><p>You can close this tab and return to the terminal.</p></body></html>")
        else:
            self.send_response(400)
            self.send_header("Content-type", "text/html")
            self.end_headers()
            self.wfile.write(b"<html><body><h1>Failed to authorize</h1></body></html>")

    def log_message(self, format, *args):
        pass # Suppress server logs

def run_server():
    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer(("", PORT), OAuthHandler) as httpd:
        print(f"[*] Temporary server listening on {REDIRECT_URI}...")
        print("[*] Please complete the authorization flow in your browser.")
        while OAuthHandler.auth_code is None:
            httpd.handle_request()
    return OAuthHandler.auth_code

def main():
    client_id, client_secret = read_config()
    
    # Build OneDrive OAuth URL
    auth_url = (
        "https://login.microsoftonline.com/common/oauth2/v2.0/authorize?"
        f"client_id={client_id}&"
        "scope=https://graph.microsoft.com/Files.ReadWrite.All%20offline_access&"
        "response_type=code&"
        f"redirect_uri={urllib.parse.quote(REDIRECT_URI)}&"
        "response_mode=query"
    )
    
    print("\n" + "="*80)
    print("1. Copy and paste this URL into your browser to log in:")
    print("="*80)
    print(auth_url)
    print("="*80 + "\n")
    
    # Try to open the browser automatically
    try:
        webbrowser.open(auth_url)
    except Exception:
        pass
    
    # Start the server to automatically catch the code when redirected to http://localhost:8080
    code = run_server()
    
    if not code:
        print("[-] Error: No authorization code received.")
        sys.exit(1)
        
    print("\n[*] Exchanging code for refresh token...")
    refresh_token = exchange_code_for_token(code, client_id, client_secret)
    
    if refresh_token:
        print(f"[+] Refresh Token: {refresh_token}")
        update_config(refresh_token)
    else:
        print("[-] Error: Did not receive a refresh token. Check your credentials and try again.")

if __name__ == "__main__":
    main()
