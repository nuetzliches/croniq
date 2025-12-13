import { Component } from '@angular/core';

import { Shell } from './shell/shell/shell';

@Component({
  selector: 'app-root',
  standalone: true,
  imports: [Shell],
  templateUrl: './app.html',
  styleUrl: './app.css'
})
export class App {
}
